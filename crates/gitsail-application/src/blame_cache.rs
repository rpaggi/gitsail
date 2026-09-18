//! Blame result cache (SAD §23 lists "expensive blame results" as a caching
//! candidate; US-034).
//!
//! Mirrors the generation-ticket pattern [`crate::session::RepositorySession`]
//! uses for status refresh (SAD §22): a cached [`Blame`] is keyed by
//! everything that could make two queries return different data — file,
//! revision, and an opaque content version the caller owns — so a hit can
//! never serve data for the wrong file, revision, or edit (US-034 criterion
//! 1). A generation counter invalidates in-flight queries the same way
//! `RepositorySession` invalidates stale refreshes: starting a newer query
//! (e.g. after switching files) makes any ticket issued before it stale, so
//! a late result can never overwrite what the newer query already applied
//! (US-034 criterion 2).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use gitsail_domain::{Blame, CommitHash};

/// Identifies one distinct blame query: everything a cached [`Blame`] must
/// match to be safely reused. `content_version` is opaque to this cache —
/// callers own how they detect "the file changed" (a content hash, an
/// mtime, an editor's own buffer revision counter, ...); it exists purely
/// so two different versions of the same file/revision never collide in the
/// cache (US-034 criterion 1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlameCacheKey {
    pub file: PathBuf,
    pub revision: Option<CommitHash>,
    pub content_version: u64,
}

/// A ticket for one in-flight blame query, tied to the generation active
/// when it was issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlameQueryTicket {
    generation: u64,
}

/// Caches blame results and guards against a late result overwriting newer
/// state (US-034).
pub struct BlameCache {
    entries: Mutex<HashMap<BlameCacheKey, Blame>>,
    generation: Mutex<u64>,
}

impl Default for BlameCache {
    fn default() -> Self {
        Self::new()
    }
}

impl BlameCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            generation: Mutex::new(0),
        }
    }

    /// A cached result for `key`, if one is present.
    pub fn get(&self, key: &BlameCacheKey) -> Option<Blame> {
        self.entries.lock().unwrap().get(key).cloned()
    }

    /// Starts a new query, bumping the generation so any ticket issued
    /// before this call is now stale (US-034 criterion 2).
    pub fn begin_query(&self) -> BlameQueryTicket {
        let mut generation = self.generation.lock().unwrap();
        *generation += 1;
        BlameQueryTicket {
            generation: *generation,
        }
    }

    /// Stores `blame` under `key` for `ticket`, unless a newer query has
    /// since started — in which case it is discarded rather than applied
    /// (US-034 criterion 2: "resposta atrasada não decora outro arquivo").
    /// Returns whether it was stored.
    pub fn complete_query(
        &self,
        ticket: BlameQueryTicket,
        key: BlameCacheKey,
        blame: Blame,
    ) -> bool {
        if ticket.generation != *self.generation.lock().unwrap() {
            return false;
        }
        self.entries.lock().unwrap().insert(key, blame);
        true
    }

    /// Drops every cached result (US-034 criterion 2: a relevant change —
    /// e.g. a commit or stage that alters history the cache might reflect —
    /// invalidates it rather than serving stale data).
    pub fn invalidate_all(&self) {
        self.entries.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::BlameOrigin;
    use std::path::Path;

    fn sample_blame(content: &str) -> Blame {
        Blame {
            file: PathBuf::from("a.txt"),
            revision: None,
            lines: vec![gitsail_domain::BlameLine {
                final_line: 1,
                original_line: 1,
                commit: CommitHash::new("deadbeef").unwrap(),
                author: gitsail_domain::Signature::new("Ada Lovelace", "ada@example.com"),
                timestamp: gitsail_domain::GitTimestamp::new(0, 0),
                content: content.to_string(),
                origin: BlameOrigin::Committed,
            }],
        }
    }

    fn key_for(file: &str, content_version: u64) -> BlameCacheKey {
        BlameCacheKey {
            file: PathBuf::from(file),
            revision: None,
            content_version,
        }
    }

    #[test]
    fn a_fresh_cache_has_no_entries() {
        let cache = BlameCache::new();
        assert!(cache.get(&key_for("a.txt", 0)).is_none());
    }

    #[test]
    fn a_completed_query_is_cached_and_retrievable() {
        let cache = BlameCache::new();
        let key = key_for("a.txt", 1);
        let ticket = cache.begin_query();

        let stored = cache.complete_query(ticket, key.clone(), sample_blame("line1"));

        assert!(stored);
        assert_eq!(cache.get(&key), Some(sample_blame("line1")));
    }

    #[test]
    fn different_content_versions_of_the_same_file_do_not_collide() {
        let cache = BlameCache::new();
        let key_v1 = key_for("a.txt", 1);
        let key_v2 = key_for("a.txt", 2);

        let ticket = cache.begin_query();
        cache.complete_query(ticket, key_v1.clone(), sample_blame("old"));
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key_v2.clone(), sample_blame("new"));

        assert_eq!(cache.get(&key_v1), Some(sample_blame("old")));
        assert_eq!(cache.get(&key_v2), Some(sample_blame("new")));
    }

    #[test]
    fn a_late_result_from_a_stale_ticket_never_overwrites_newer_state() {
        let cache = BlameCache::new();
        let stale_key = key_for("a.txt", 1);
        let fresh_key = key_for("b.txt", 1);

        // Simulates a rapid file switch: a query for a.txt starts, then the
        // user switches to b.txt before the first one returns.
        let stale_ticket = cache.begin_query();
        let fresh_ticket = cache.begin_query();
        cache.complete_query(fresh_ticket, fresh_key.clone(), sample_blame("b-content"));

        let stored =
            cache.complete_query(stale_ticket, stale_key.clone(), sample_blame("a-content"));

        assert!(!stored, "a stale ticket must not populate the cache");
        assert!(cache.get(&stale_key).is_none());
        assert_eq!(cache.get(&fresh_key), Some(sample_blame("b-content")));
    }

    #[test]
    fn invalidate_all_drops_every_entry() {
        let cache = BlameCache::new();
        let key = key_for(Path::new("a.txt").to_str().unwrap(), 1);
        let ticket = cache.begin_query();
        cache.complete_query(ticket, key.clone(), sample_blame("line1"));
        assert!(cache.get(&key).is_some());

        cache.invalidate_all();

        assert!(cache.get(&key).is_none());
    }
}
