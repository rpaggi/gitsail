//! Recent repositories list (US-052).
//!
//! Pure list-management logic only — no filesystem or platform I/O — so it
//! can be reused by any GitSail frontend (TUI, CLI, VS Code) that wants a
//! "recently opened" shortcut, not just Desktop. Persistence is a port
//! ([`RecentRepositoriesPort`]); the concrete disk format is an adapter
//! concern (e.g. Desktop's own JSON file store) that lives outside this
//! crate, keeping the dependency direction inward (AGENTS.md).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitsail_domain::GitSailError;

/// The most entries [`RecentRepositories`] keeps. Touching a repository
/// beyond this cap evicts the least-recently-opened entry (US-052
/// criterion 1: "keeps a list", not an unbounded one).
pub const MAX_RECENT_REPOSITORIES: usize = 10;

/// One remembered repository: where it is, and when it was last opened.
///
/// `path` is expected to be the *canonical* root path a successful
/// `OpenRepository` use case resolved to (knowledge base rule: "opening a
/// repository by current directory, root, or subdirectory must resolve to
/// the same repository") — a caller must never [`RecentRepositories::touch`]
/// a raw, unresolved user-typed path, or the same repository could appear
/// twice under two different spellings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRepositoryEntry {
    pub path: PathBuf,
    pub last_opened_unix_seconds: i64,
}

/// A bounded, most-recently-opened-first list of [`RecentRepositoryEntry`]
/// (US-052 criterion 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentRepositories {
    entries: Vec<RecentRepositoryEntry>,
}

impl RecentRepositories {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a list from previously persisted entries, re-applying the
    /// same dedupe-by-path/cap/most-recent-first invariants
    /// [`Self::touch`] enforces — so a store loading a file written by an
    /// older version (or hand-edited) never hands back a list that
    /// violates them.
    pub fn from_entries(entries: Vec<RecentRepositoryEntry>) -> Self {
        let mut ordered = entries;
        // Oldest first, so replaying a `touch` per entry ends with the
        // most-recently-opened entry at the front, matching the original
        // recency order regardless of the order entries were stored in.
        ordered.sort_by_key(|entry| entry.last_opened_unix_seconds);

        let mut recents = Self::new();
        for entry in ordered {
            recents.touch(entry.path, entry.last_opened_unix_seconds);
        }
        recents
    }

    pub fn entries(&self) -> &[RecentRepositoryEntry] {
        &self.entries
    }

    /// Records `path` as just opened: moves it to the front (updating its
    /// timestamp) if already present, inserts it at the front otherwise,
    /// and evicts the oldest entry past [`MAX_RECENT_REPOSITORIES`]
    /// (US-052 criterion 1).
    pub fn touch(&mut self, path: PathBuf, now_unix_seconds: i64) {
        self.entries.retain(|entry| entry.path != path);
        self.entries.insert(
            0,
            RecentRepositoryEntry {
                path,
                last_opened_unix_seconds: now_unix_seconds,
            },
        );
        self.entries.truncate(MAX_RECENT_REPOSITORIES);
    }

    /// Removes `path` from the list, if present. A no-op otherwise — the
    /// caller (US-052 criterion 2: "never erase without confirmation")
    /// decides *when* this is called, never this type.
    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|entry| entry.path != path);
    }
}

/// Persists a [`RecentRepositories`] list across process restarts (US-052
/// criterion 1). Adapters decide the concrete storage: Desktop uses a JSON
/// file under the OS config directory (see `apps/desktop/src-tauri`); a
/// future TUI/CLI adapter could implement this same trait against its own
/// location.
pub trait RecentRepositoriesPort: Send + Sync {
    fn load(&self) -> Result<RecentRepositories, GitSailError>;
    fn save(&self, recents: &RecentRepositories) -> Result<(), GitSailError>;
}

/// Lists the currently remembered repositories (US-052 criterion 1).
pub struct ListRecentRepositories {
    port: Arc<dyn RecentRepositoriesPort>,
}

impl ListRecentRepositories {
    pub fn new(port: Arc<dyn RecentRepositoriesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self) -> Result<RecentRepositories, GitSailError> {
        self.port.load()
    }
}

/// Records that `path` was just opened, persisting the updated list
/// (US-052 criterion 1).
pub struct RecordRecentRepository {
    port: Arc<dyn RecentRepositoriesPort>,
}

impl RecordRecentRepository {
    pub fn new(port: Arc<dyn RecentRepositoriesPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        path: PathBuf,
        now_unix_seconds: i64,
    ) -> Result<RecentRepositories, GitSailError> {
        let mut recents = self.port.load()?;
        recents.touch(path, now_unix_seconds);
        self.port.save(&recents)?;
        Ok(recents)
    }
}

/// Removes `path` from the remembered list, persisting the result (US-052
/// criterion 2: the explicit "remove from list" confirmation this backs).
pub struct ForgetRecentRepository {
    port: Arc<dyn RecentRepositoriesPort>,
}

impl ForgetRecentRepository {
    pub fn new(port: Arc<dyn RecentRepositoriesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, path: &Path) -> Result<RecentRepositories, GitSailError> {
        let mut recents = self.port.load()?;
        recents.remove(path);
        self.port.save(&recents)?;
        Ok(recents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::ErrorCode;
    use std::sync::Mutex;

    /// A [`RecentRepositoriesPort`] double backed by memory, with switches
    /// to force `load`/`save` failures so use cases can be exercised
    /// against a misbehaving store without any real file I/O.
    struct FakeStore {
        recents: Mutex<RecentRepositories>,
        fail_load: bool,
        fail_save: bool,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                recents: Mutex::new(RecentRepositories::new()),
                fail_load: false,
                fail_save: false,
            }
        }
    }

    impl RecentRepositoriesPort for FakeStore {
        fn load(&self) -> Result<RecentRepositories, GitSailError> {
            if self.fail_load {
                return Err(GitSailError::new(ErrorCode::Internal, "load failed"));
            }
            Ok(self.recents.lock().unwrap().clone())
        }

        fn save(&self, recents: &RecentRepositories) -> Result<(), GitSailError> {
            if self.fail_save {
                return Err(GitSailError::new(ErrorCode::Internal, "save failed"));
            }
            *self.recents.lock().unwrap() = recents.clone();
            Ok(())
        }
    }

    #[test]
    fn touch_inserts_a_new_entry_at_the_front() {
        let mut recents = RecentRepositories::new();

        recents.touch(PathBuf::from("/repo-a"), 100);
        recents.touch(PathBuf::from("/repo-b"), 200);

        let paths: Vec<&Path> = recents.entries().iter().map(|e| e.path.as_path()).collect();
        assert_eq!(paths, vec![Path::new("/repo-b"), Path::new("/repo-a")]);
    }

    #[test]
    fn touching_an_existing_path_moves_it_to_front_and_updates_its_timestamp() {
        let mut recents = RecentRepositories::new();
        recents.touch(PathBuf::from("/repo-a"), 100);
        recents.touch(PathBuf::from("/repo-b"), 200);

        recents.touch(PathBuf::from("/repo-a"), 300);

        let paths: Vec<&Path> = recents.entries().iter().map(|e| e.path.as_path()).collect();
        assert_eq!(
            paths,
            vec![Path::new("/repo-a"), Path::new("/repo-b")],
            "re-touching a path must move it to the front rather than duplicating it"
        );
        assert_eq!(recents.entries()[0].last_opened_unix_seconds, 300);
        assert_eq!(recents.entries().len(), 2, "no duplicate entry for the same path");
    }

    #[test]
    fn touch_evicts_the_oldest_entry_once_the_cap_is_exceeded() {
        let mut recents = RecentRepositories::new();
        for i in 0..MAX_RECENT_REPOSITORIES {
            recents.touch(PathBuf::from(format!("/repo-{i}")), i as i64);
        }
        assert_eq!(recents.entries().len(), MAX_RECENT_REPOSITORIES);

        recents.touch(PathBuf::from("/repo-new"), 1_000);

        assert_eq!(recents.entries().len(), MAX_RECENT_REPOSITORIES, "the list must never grow past the cap");
        assert!(
            recents.entries().iter().all(|e| e.path != Path::new("/repo-0")),
            "the least-recently-opened entry must be the one evicted"
        );
        assert_eq!(recents.entries()[0].path, Path::new("/repo-new"));
    }

    #[test]
    fn remove_drops_the_matching_entry_and_is_a_no_op_otherwise() {
        let mut recents = RecentRepositories::new();
        recents.touch(PathBuf::from("/repo-a"), 100);
        recents.touch(PathBuf::from("/repo-b"), 200);

        recents.remove(Path::new("/repo-a"));
        assert_eq!(recents.entries().len(), 1);
        assert_eq!(recents.entries()[0].path, Path::new("/repo-b"));

        // US-052 criterion 2: removing an absent path must never panic or
        // otherwise disturb the rest of the list.
        recents.remove(Path::new("/does-not-exist"));
        assert_eq!(recents.entries().len(), 1);
    }

    #[test]
    fn from_entries_normalizes_order_and_re_applies_the_dedupe_and_cap_invariants() {
        // Given out of (recency) order and with a duplicate path, as a
        // hand-edited or older-schema file might contain.
        let stored = vec![
            RecentRepositoryEntry { path: PathBuf::from("/repo-a"), last_opened_unix_seconds: 100 },
            RecentRepositoryEntry { path: PathBuf::from("/repo-b"), last_opened_unix_seconds: 300 },
            RecentRepositoryEntry { path: PathBuf::from("/repo-a"), last_opened_unix_seconds: 200 },
        ];

        let recents = RecentRepositories::from_entries(stored);

        let paths: Vec<&Path> = recents.entries().iter().map(|e| e.path.as_path()).collect();
        assert_eq!(
            paths,
            vec![Path::new("/repo-b"), Path::new("/repo-a")],
            "most-recently-opened first, with the duplicate collapsed to its latest timestamp"
        );
    }

    #[test]
    fn list_use_case_delegates_to_the_port() {
        let store = Arc::new(FakeStore::new());
        store.save(&{
            let mut r = RecentRepositories::new();
            r.touch(PathBuf::from("/repo"), 1);
            r
        }).unwrap();

        let recents = ListRecentRepositories::new(store).execute().unwrap();

        assert_eq!(recents.entries().len(), 1);
    }

    #[test]
    fn list_use_case_propagates_a_load_failure() {
        let mut store = FakeStore::new();
        store.fail_load = true;
        let err = ListRecentRepositories::new(Arc::new(store)).execute().unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn record_use_case_loads_touches_and_persists_through_the_port() {
        let store = Arc::new(FakeStore::new());

        let recents = RecordRecentRepository::new(store.clone())
            .execute(PathBuf::from("/repo"), 42)
            .unwrap();

        assert_eq!(recents.entries().len(), 1);
        assert_eq!(recents.entries()[0].last_opened_unix_seconds, 42);
        // The use case must have persisted, not just returned, the update.
        assert_eq!(store.load().unwrap().entries().len(), 1);
    }

    #[test]
    fn record_use_case_propagates_a_save_failure_without_silently_succeeding() {
        let mut store = FakeStore::new();
        store.fail_save = true;
        let err = RecordRecentRepository::new(Arc::new(store))
            .execute(PathBuf::from("/repo"), 1)
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn forget_use_case_loads_removes_and_persists_through_the_port() {
        let store = Arc::new(FakeStore::new());
        RecordRecentRepository::new(store.clone())
            .execute(PathBuf::from("/repo"), 1)
            .unwrap();

        let recents = ForgetRecentRepository::new(store.clone())
            .execute(Path::new("/repo"))
            .unwrap();

        assert!(recents.entries().is_empty());
        assert!(store.load().unwrap().entries().is_empty());
    }
}
