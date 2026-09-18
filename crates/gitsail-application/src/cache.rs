//! Generic, bounded, generation-guarded cache (SAD §23; T-228/US-117).
//!
//! Generalizes the pattern [`crate::blame_cache::BlameCache`] (EPIC-07)
//! pioneered for blame specifically — key by everything that could make two
//! queries return different data, guard against a late result overwriting
//! newer state via a generation ticket, and never grow without bound — so a
//! second cache ([`crate::graph_cache::GraphCache`], for the commit graph)
//! does not reimplement any of that from scratch.
//!
//! `BlameCache` itself is deliberately *not* rewritten to sit on top of this
//! type: it is already correct, already has its own passing test suite, and
//! migrating it would be a mechanical, test-risking rewrite for no
//! behavioral gain (US-117's own wording allows this: "generalize the
//! existing `BlameCache` pattern ... or build a generic cache both can use,
//! if this is not disproportionate rework" — rewriting a working module in
//! place is the disproportionate option; building the generic cache fresh
//! and using it for the *new* consumer is not). Any future cache should
//! build on `GenerationCache`, not copy `BlameCache`'s code again.
//!
//! # Cache is never the source of truth
//!
//! SAD §23: "cache is an optimization, never the source of truth." This
//! cache (and [`crate::graph_cache::GraphCache`] built on it) only ever
//! stores *read* results (graph pages today). A destructive/risky
//! decision must never be made from a cached value instead of revalidating
//! against the real repository — `RepositoryWritePort::amend_commit`'s
//! `expected_head` check already revalidates HEAD against a fresh
//! `resolve_revision` call, never a cache, and that principle is meant to
//! extend to any future mutation with the same preview/execute race window
//! (T-228/US-117 criterion 3).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

/// A ticket for one in-flight query, tied to the generation active when it
/// was issued — the same discipline [`crate::session::RepositorySession`]
/// (SAD §22) and [`crate::blame_cache::BlameCache`] (US-034) each already
/// implement independently, factored out here so a new cache reuses it
/// instead of reimplementing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationTicket(u64);

/// A bounded, generation-guarded cache.
///
/// Entries are keyed by `K` — a caller composes into it everything that
/// could make two queries return different data (repository, filter, page —
/// T-228/US-117 criterion 1). A generation ticket guards against a late
/// query result overwriting a newer one: starting a newer query (via
/// [`Self::begin_query`]) makes any ticket issued before it stale, so
/// [`Self::complete_query`] silently drops a late result instead of
/// applying it — mirroring [`crate::blame_cache::BlameCache`]'s existing
/// guarantee for blame specifically.
///
/// Bounded by `capacity`: once full, inserting a new entry evicts the
/// least-recently-inserted entry first (a simple FIFO bound, not a full
/// LRU) — sufficient to guarantee the cache never grows without bound,
/// which is the actual requirement (T-228/US-117 DoD: "um teste comprova
/// limites/evicção"); a recency-aware eviction policy is a possible future
/// refinement, not required by any criterion here.
pub struct GenerationCache<K, V> {
    entries: Mutex<HashMap<K, V>>,
    order: Mutex<VecDeque<K>>,
    generation: Mutex<u64>,
    capacity: usize,
}

impl<K, V> GenerationCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// `capacity` is clamped to at least 1: a cache holding nothing would
    /// not be a cache, only a more expensive way to always miss.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            order: Mutex::new(VecDeque::new()),
            generation: Mutex::new(0),
            capacity: capacity.max(1),
        }
    }

    /// A cached result for `key`, if one is present and has not been
    /// invalidated.
    pub fn get(&self, key: &K) -> Option<V> {
        self.entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(key)
            .cloned()
    }

    /// Starts a new query, bumping the generation so any ticket issued
    /// before this call is now stale (T-228/US-117 criterion 2).
    pub fn begin_query(&self) -> GenerationTicket {
        let mut generation = self.generation.lock().unwrap_or_else(|poison| poison.into_inner());
        *generation += 1;
        GenerationTicket(*generation)
    }

    /// Stores `value` under `key` for `ticket`, unless a newer query has
    /// since started — in which case it is discarded rather than applied.
    /// Returns whether it was stored. Evicts the oldest entry first if this
    /// insertion would exceed `capacity`.
    pub fn complete_query(&self, ticket: GenerationTicket, key: K, value: V) -> bool {
        if ticket.0 != *self.generation.lock().unwrap_or_else(|poison| poison.into_inner()) {
            return false;
        }
        let mut entries = self.entries.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut order = self.order.lock().unwrap_or_else(|poison| poison.into_inner());
        if !entries.contains_key(&key) {
            order.push_back(key.clone());
        }
        entries.insert(key, value);
        while entries.len() > self.capacity {
            match order.pop_front() {
                Some(oldest) => {
                    entries.remove(&oldest);
                }
                None => break,
            }
        }
        true
    }

    /// Drops every cached entry (SAD §26: manual refresh, a ref change, or
    /// any mutation all invalidate a relevant cache this same way).
    pub fn invalidate_all(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        self.order
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u32) -> String {
        format!("key-{n}")
    }

    #[test]
    fn a_fresh_cache_has_no_entries() {
        let cache: GenerationCache<String, u32> = GenerationCache::new(4);
        assert!(cache.get(&key(1)).is_none());
    }

    #[test]
    fn a_completed_query_is_cached_and_retrievable() {
        let cache = GenerationCache::new(4);
        let ticket = cache.begin_query();

        let stored = cache.complete_query(ticket, key(1), 100u32);

        assert!(stored);
        assert_eq!(cache.get(&key(1)), Some(100));
    }

    #[test]
    fn different_keys_do_not_collide() {
        let cache = GenerationCache::new(4);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key(1), 1u32);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key(2), 2u32);

        assert_eq!(cache.get(&key(1)), Some(1));
        assert_eq!(cache.get(&key(2)), Some(2));
    }

    #[test]
    fn a_late_result_from_a_stale_ticket_never_overwrites_newer_state() {
        let cache = GenerationCache::new(4);
        let stale_ticket = cache.begin_query();
        let fresh_ticket = cache.begin_query();
        cache.complete_query(fresh_ticket, key(2), 2u32);

        let stored = cache.complete_query(stale_ticket, key(1), 1u32);

        assert!(!stored, "a stale ticket must not populate the cache");
        assert!(cache.get(&key(1)).is_none());
        assert_eq!(cache.get(&key(2)), Some(2));
    }

    #[test]
    fn invalidate_all_drops_every_entry() {
        let cache = GenerationCache::new(4);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key(1), 1u32);
        assert!(cache.get(&key(1)).is_some());

        cache.invalidate_all();

        assert!(cache.get(&key(1)).is_none());
    }

    /// T-228/US-117 DoD: "um teste comprova limites/evicção (o cache não
    /// cresce sem limite)".
    #[test]
    fn the_cache_never_grows_past_its_capacity() {
        let cache = GenerationCache::new(3);

        for n in 0..10 {
            let ticket = cache.begin_query();
            cache.complete_query(ticket, key(n), n);
        }

        assert_eq!(cache.len(), 3);
        // The most recently inserted entries survive; the oldest are gone.
        assert_eq!(cache.get(&key(9)), Some(9));
        assert!(cache.get(&key(0)).is_none());
    }

    #[test]
    fn capacity_is_never_zero_even_if_requested() {
        let cache = GenerationCache::new(0);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key(1), 1u32);
        assert_eq!(cache.get(&key(1)), Some(1));
    }
}
