//! Cache for paginated commit-graph pages (SAD §23 lists "graph layout"
//! among caching candidates; T-228/US-117), built on
//! [`crate::cache::GenerationCache`] — the same generation-ticket/eviction
//! mechanism [`crate::blame_cache::BlameCache`] (EPIC-07) already applies to
//! blame, applied here to [`gitsail_domain::graph::CommitGraph`]'s paginated
//! rows (US-065/EPIC-13) instead.
//!
//! [`gitsail_domain::graph::CommitGraph`] itself holds no notion of caching,
//! generation, or which repository/filter it belongs to (by design — it is
//! pure incremental-layout data, SAD's dependency rule keeps domain code
//! free of that kind of session/cache concern). This module is the
//! Core-level layer that adds those concerns on top, so a caller — TUI,
//! Desktop, or a future consumer — can reuse a previously computed page of
//! [`GraphRow`]s instead of recomputing/re-fetching it, while still being
//! told (via [`GraphCache::begin_query`]/[`GraphCache::complete_query`])
//! when a result has gone stale.

use gitsail_domain::{GraphRow, RepositoryId};

use crate::cache::{GenerationCache, GenerationTicket};
use crate::concurrency::Invalidatable;

/// Default number of distinct graph-page queries kept cached at once,
/// chosen generously enough to cover a session paging back and forth
/// through recent history without growing without bound (T-228/US-117 DoD:
/// "o cache não cresce sem limite").
pub const DEFAULT_GRAPH_CACHE_CAPACITY: usize = 64;

/// Identifies one distinct graph-page query: everything a cached page of
/// [`GraphRow`]s must match to be safely reused (T-228/US-117 criterion 1:
/// "chaves que incluem contexto (repositório, filtro, página)").
///
/// `filter_signature` is opaque to this cache, mirroring
/// [`crate::blame_cache::BlameCacheKey::content_version`]'s design: callers
/// own how they summarize "which query" (a branch/revision-range/path
/// filter combination) into one comparable value; this cache only needs it
/// to distinguish two different queries against the same repository from
/// each other.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphPageKey {
    pub repository: RepositoryId,
    pub filter_signature: String,
    /// The page cursor this key was fetched for (`None` for the first
    /// page), continuing US-065's paginated `CommitGraph::append_page`.
    pub cursor: Option<String>,
}

/// Caches paginated [`GraphRow`] results and guards against a late result
/// overwriting newer state, exactly like [`crate::blame_cache::BlameCache`]
/// does for blame (US-034) — generalized here via
/// [`crate::cache::GenerationCache`] (T-228/US-117).
pub struct GraphCache {
    inner: GenerationCache<GraphPageKey, Vec<GraphRow>>,
}

impl Default for GraphCache {
    fn default() -> Self {
        Self::new(DEFAULT_GRAPH_CACHE_CAPACITY)
    }
}

impl GraphCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: GenerationCache::new(capacity),
        }
    }

    /// A cached page for `key`, if one is present.
    pub fn get(&self, key: &GraphPageKey) -> Option<Vec<GraphRow>> {
        self.inner.get(key)
    }

    /// Starts a new graph-page query, bumping the generation so any ticket
    /// issued before this call is now stale (T-228/US-117 criterion 2: a
    /// ref change — new commit, branch switch — or a manual refresh must
    /// invalidate rather than let a late in-flight fetch silently overwrite
    /// what a newer one already applied).
    pub fn begin_query(&self) -> GenerationTicket {
        self.inner.begin_query()
    }

    /// Stores `rows` under `key` for `ticket`, unless a newer query has
    /// since started. Returns whether it was stored.
    pub fn complete_query(&self, ticket: GenerationTicket, key: GraphPageKey, rows: Vec<GraphRow>) -> bool {
        self.inner.complete_query(ticket, key, rows)
    }

    /// Drops every cached page (T-228/US-117 criterion 2: refresh, a ref
    /// change, or any mutation all invalidate this way — never served a
    /// stale page "without the consumer knowing").
    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }
}

/// Lets [`crate::session::RepositorySession`] drive this cache's
/// invalidation through the same uniform mechanism it uses for
/// [`crate::blame_cache::BlameCache`], after a successful mutation
/// (T-227/US-116 criterion 3).
impl Invalidatable for GraphCache {
    fn invalidate_all(&self) {
        GraphCache::invalidate_all(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::CommitHash;
    use std::path::Path;

    fn sample_row(hash: &str) -> GraphRow {
        GraphRow {
            commit: CommitHash::new(hash).unwrap(),
            lane: 0,
            decorations: vec![],
            edges: vec![],
            passthrough_lanes: vec![],
        }
    }

    fn key(page: &str) -> GraphPageKey {
        GraphPageKey {
            repository: RepositoryId::from_canonical_root(Path::new("/repo")),
            filter_signature: "branch=main".to_string(),
            cursor: Some(page.to_string()),
        }
    }

    #[test]
    fn a_completed_query_is_cached_and_retrievable() {
        let cache = GraphCache::default();
        let ticket = cache.begin_query();
        let rows = vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")];

        let stored = cache.complete_query(ticket, key("0"), rows.clone());

        assert!(stored);
        assert_eq!(cache.get(&key("0")), Some(rows));
    }

    /// T-228/US-117 DoD: "teste de cache hit seguido de uma mudança externa
    /// ao repositório ... comprova que a próxima consulta não serve o dado
    /// obsoleto". Simulated here the same way `BlameCache`'s equivalent test
    /// is: an external change is modeled as "the consumer calls
    /// `invalidate_all`" (what `RepositorySession::run_mutation`/a ref-change
    /// detection would trigger in practice — see T-227's wiring), and the
    /// next query result never blends with the stale one.
    #[test]
    fn a_cache_hit_never_survives_an_external_change_invalidation() {
        let cache = GraphCache::default();
        let ticket = cache.begin_query();
        let old_rows = vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")];
        cache.complete_query(ticket, key("0"), old_rows.clone());
        assert_eq!(cache.get(&key("0")), Some(old_rows));

        // An external process (or a GitSail mutation) changes the
        // repository; the consumer invalidates the cache in response.
        cache.invalidate_all();

        assert!(cache.get(&key("0")).is_none());

        // The next query for the same key gets a fresh generation and a
        // fresh result — never quietly reusing what was cleared.
        let ticket = cache.begin_query();
        let new_rows = vec![sample_row("cafef00dcafef00dcafef00dcafef00dcafef00")];
        cache.complete_query(ticket, key("0"), new_rows.clone());
        assert_eq!(cache.get(&key("0")), Some(new_rows));
    }

    #[test]
    fn a_late_result_from_a_stale_ticket_never_overwrites_a_newer_page() {
        let cache = GraphCache::default();
        let stale_ticket = cache.begin_query();
        let fresh_ticket = cache.begin_query();
        let fresh_rows = vec![sample_row("cafef00dcafef00dcafef00dcafef00dcafef00")];
        cache.complete_query(fresh_ticket, key("0"), fresh_rows.clone());

        let stored = cache.complete_query(
            stale_ticket,
            key("0"),
            vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")],
        );

        assert!(!stored);
        assert_eq!(cache.get(&key("0")), Some(fresh_rows));
    }

    #[test]
    fn keys_differing_only_by_page_cursor_do_not_collide() {
        let cache = GraphCache::default();
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key("0"), vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")]);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key("50"), vec![sample_row("cafef00dcafef00dcafef00dcafef00dcafef00")]);

        assert_ne!(cache.get(&key("0")), cache.get(&key("50")));
    }

    #[test]
    fn the_cache_never_grows_past_its_capacity() {
        let cache = GraphCache::new(2);
        for n in 0..5 {
            let ticket = cache.begin_query();
            cache.complete_query(ticket, key(&n.to_string()), vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")]);
        }
        assert!(cache.get(&key("4")).is_some());
        assert!(cache.get(&key("0")).is_none());
    }

    #[test]
    fn invalidatable_trait_object_drives_the_same_invalidation() {
        let cache = GraphCache::default();
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key("0"), vec![sample_row("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")]);

        let as_trait_object: &dyn Invalidatable = &cache;
        as_trait_object.invalidate_all();

        assert!(cache.get(&key("0")).is_none());
    }
}
