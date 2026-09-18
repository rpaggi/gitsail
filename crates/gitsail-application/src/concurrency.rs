//! Per-repository mutation serialization and mutation-triggered cache
//! invalidation (SAD §26; T-227/US-116).
//!
//! SAD §26 states the policy in prose ("mutations against the same
//! repository are serialized unless proven safe"; "a mutation triggers
//! invalidation of relevant read caches") without naming a concrete
//! mechanism. Before this module, nothing in the workspace implemented
//! either half of that policy anywhere: a single `RepositoryWritePort` call
//! only ever runs one `git` process at a time (so it cannot corrupt the
//! index *by itself*), but nothing coordinated *across* concurrent calls —
//! from multiple `RepositorySession`s in the same process, e.g. two linked
//! worktrees opened independently, or in principle multiple processes
//! sharing a repository (`gitsail-application/src/write_ports.rs`'s module
//! doc explicitly named this exact gap as EPIC-23/US-116's job). This
//! module is that mechanism, generalized in Core so `gitsail-tui` and
//! `apps/desktop` stop needing to hand-roll their own (see ADR-019).
//!
//! # What this does and does not coordinate
//!
//! [`RepositoryLockRegistry`] serializes **in-process** mutation attempts
//! that resolve to the same lock key (see
//! [`crate::ports::RepositoryReadPort::lock_key`] and ADR-019 for why that
//! key is the repository's real, shared `.git` directory rather than its
//! worktree path). It does **not** replace Git's own `.git/index.lock`:
//! that file is Git's cross-process guard (another `git` invocation, or a
//! GitSail process on a different machine/container sharing the same
//! filesystem, is entirely outside this registry's reach). When GitSail's
//! own mutation loses that race anyway, `gitsail-git`'s
//! `classify_index_lock_conflict` turns Git's refusal into
//! [`gitsail_domain::ErrorCode::RepositoryLocked`] — a clear, classified
//! error — instead of hanging or corrupting anything; GitSail never waits
//! for or removes another process's lock file itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// A cache that can be told "the repository changed, forget everything you
/// know" without needing to understand *why* (SAD §26; T-228/US-117).
/// Implemented by [`crate::blame_cache::BlameCache`] and
/// [`crate::graph_cache::GraphCache`] so [`crate::session::RepositorySession`]
/// can drive every registered cache's invalidation through one uniform
/// mechanism after a successful mutation, instead of each mutation use case
/// having to know the full list of caches that might now be stale.
pub trait Invalidatable: Send + Sync {
    fn invalidate_all(&self);
}

/// Process-wide registry of per-repository mutation locks, keyed by
/// [`crate::ports::RepositoryReadPort::lock_key`]'s result for a given
/// repository. Every [`crate::session::RepositorySession`] in a process
/// resolves its lock through the same [`global_lock_registry`], so two
/// sessions targeting the same physical repository — even via two
/// different `RepositorySession` instances, e.g. two linked worktrees, or
/// simply the same path opened twice — share one `Arc<Mutex<()>>` and are
/// serialized against each other, not only against themselves.
#[derive(Default)]
pub struct RepositoryLockRegistry {
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl RepositoryLockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared mutex for `key`, creating one on first use. Every caller
    /// resolving the same `key` receives a clone of the same `Arc`.
    ///
    /// Entries are never evicted: a repository closed and reopened later
    /// reuses its lock rather than racing a fresh one into existence, and
    /// the number of distinct repositories a long-lived process ever opens
    /// is small enough that this is not a practical growth concern (unlike
    /// [`crate::cache::GenerationCache`], which bounds *query result*
    /// growth for exactly this reason).
    pub fn lock_for(&self, key: &Path) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|poison| poison.into_inner());
        locks
            .entry(key.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Number of distinct repositories with an allocated lock. Exposed for
    /// tests; not a capability presentation layers need.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.locks.lock().unwrap().len()
    }
}

static LOCK_REGISTRY: OnceLock<RepositoryLockRegistry> = OnceLock::new();

/// The process-wide [`RepositoryLockRegistry`] every
/// [`crate::session::RepositorySession`] shares. A single process-wide
/// static (rather than a registry threaded through every constructor call)
/// is the deliberate design here: it is exactly what lets `gitsail-tui` and
/// `apps/desktop` get "mutations against the same repository are
/// serialized" for free, without either frontend having to construct,
/// thread through, and share a registry object itself (see ADR-019).
pub fn global_lock_registry() -> &'static RepositoryLockRegistry {
    LOCK_REGISTRY.get_or_init(RepositoryLockRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_key_always_yields_the_same_lock() {
        let registry = RepositoryLockRegistry::new();
        let a = registry.lock_for(Path::new("/repo/.git"));
        let b = registry.lock_for(Path::new("/repo/.git"));
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn different_keys_yield_different_locks() {
        let registry = RepositoryLockRegistry::new();
        let a = registry.lock_for(Path::new("/repo-a/.git"));
        let b = registry.lock_for(Path::new("/repo-b/.git"));
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn global_lock_registry_is_a_single_shared_instance() {
        let a = global_lock_registry().lock_for(Path::new("/somewhere/.git"));
        let b = global_lock_registry().lock_for(Path::new("/somewhere/.git"));
        assert!(Arc::ptr_eq(&a, &b));
    }

    /// Real OS threads (not sequential simulation) race to create a lock for
    /// the same key; every one of them must end up sharing the one instance
    /// the registry ever creates for that key, never a duplicate.
    #[test]
    fn concurrent_first_access_never_creates_duplicate_locks() {
        let registry = Arc::new(RepositoryLockRegistry::new());
        let key = PathBuf::from("/racing-repo/.git");

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let registry = Arc::clone(&registry);
                let key = key.clone();
                std::thread::spawn(move || registry.lock_for(&key))
            })
            .collect();

        let locks: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let first = &locks[0];
        assert!(locks.iter().all(|lock| Arc::ptr_eq(lock, first)));
        assert_eq!(registry.len(), 1);
    }
}
