//! Desktop's disk persistence for the recent-repositories list (US-052
//! criterion 1).
//!
//! This is the *only* Desktop-specific piece of US-052: the list logic
//! (dedupe, cap, most-recent-first ordering) lives in
//! `gitsail_application::recent_repositories` so it can be reused by a
//! future TUI/CLI adapter; this module only implements
//! [`RecentRepositoriesPort`] against a JSON file on disk.
//!
//! **Persistence decision**: the file lives at
//! `<OS config dir>/gitsail/desktop/recent-repositories.json` (e.g.
//! `~/.config/gitsail/desktop/recent-repositories.json` on Linux,
//! `~/Library/Application Support/gitsail/desktop/recent-repositories.json`
//! on macOS, `%APPDATA%\gitsail\desktop\recent-repositories.json` on
//! Windows), resolved via the `dirs` crate (already vendored transitively
//! through `tauri` itself, so this adds no new external dependency
//! surface). A plain JSON file was chosen over a database or Tauri's own
//! store plugin because the data is tiny (at most
//! [`gitsail_application::MAX_RECENT_REPOSITORIES`] entries), human
//! readable for debugging, and needs no query capability beyond "load the
//! whole list, save the whole list" — exactly what
//! `RecentRepositoriesPort` asks for.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use gitsail_application::{RecentRepositories, RecentRepositoriesPort, RecentRepositoryEntry};
use gitsail_domain::{ErrorCode, GitSailError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct StoredEntry {
    path: PathBuf,
    last_opened_unix_seconds: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredFile {
    #[serde(default)]
    entries: Vec<StoredEntry>,
}

/// A [`RecentRepositoriesPort`] backed by a single JSON file.
///
/// Guarded by its own [`Mutex`] (in addition to whatever `AppState` does
/// with the returned list) so two Tauri command invocations racing to
/// `load()`-then-`save()` a `RecentRepositories` never interleave their
/// file reads/writes — a "load, mutate, save" round trip elsewhere
/// (`gitsail_application::RecordRecentRepository`/`ForgetRecentRepository`)
/// is otherwise not atomic across two separate port calls.
pub struct JsonFileRecentRepositoriesStore {
    file_path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileRecentRepositoriesStore {
    pub fn new(file_path: PathBuf) -> Self {
        Self { file_path, lock: Mutex::new(()) }
    }

    /// The default file location for this platform (see the module
    /// documentation's persistence decision).
    pub fn default_location() -> Result<PathBuf, GitSailError> {
        let config_dir = dirs::config_dir().ok_or_else(|| {
            GitSailError::new(
                ErrorCode::Internal,
                "could not resolve the OS configuration directory",
            )
        })?;
        Ok(config_dir.join("gitsail").join("desktop").join("recent-repositories.json"))
    }

    fn read_error(err: std::io::Error) -> GitSailError {
        GitSailError::new(ErrorCode::Internal, "failed to read the recent repositories file")
            .with_source(err)
    }

    fn write_error(err: std::io::Error) -> GitSailError {
        GitSailError::new(ErrorCode::Internal, "failed to write the recent repositories file")
            .with_source(err)
    }

    fn parse_error(err: serde_json::Error) -> GitSailError {
        GitSailError::new(ErrorCode::ParseFailure, "the recent repositories file is corrupted")
            .with_source(err)
    }
}

impl RecentRepositoriesPort for JsonFileRecentRepositoriesStore {
    fn load(&self) -> Result<RecentRepositories, GitSailError> {
        let _guard = self.lock.lock().expect("recent repositories store mutex poisoned");

        if !self.file_path.exists() {
            return Ok(RecentRepositories::new());
        }
        let contents = fs::read_to_string(&self.file_path).map_err(Self::read_error)?;
        if contents.trim().is_empty() {
            return Ok(RecentRepositories::new());
        }
        let stored: StoredFile = serde_json::from_str(&contents).map_err(Self::parse_error)?;
        let entries = stored
            .entries
            .into_iter()
            .map(|e| RecentRepositoryEntry {
                path: e.path,
                last_opened_unix_seconds: e.last_opened_unix_seconds,
            })
            .collect();
        Ok(RecentRepositories::from_entries(entries))
    }

    fn save(&self, recents: &RecentRepositories) -> Result<(), GitSailError> {
        let _guard = self.lock.lock().expect("recent repositories store mutex poisoned");

        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(Self::write_error)?;
        }
        let stored = StoredFile {
            entries: recents
                .entries()
                .iter()
                .map(|e| StoredEntry {
                    path: e.path.clone(),
                    last_opened_unix_seconds: e.last_opened_unix_seconds,
                })
                .collect(),
        };
        let json = serde_json::to_string_pretty(&stored).map_err(Self::parse_error)?;
        fs::write(&self.file_path, json).map_err(Self::write_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A fresh, unique file path under the OS temp directory, cleaned up by
    /// the caller (`Drop` is deliberately not used, so a failed assertion
    /// leaves the file on disk for inspection).
    fn temp_file_path() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "gitsail-recent-repositories-test-{}-{id}.json",
            std::process::id()
        ))
    }

    #[test]
    fn load_without_a_file_yet_returns_an_empty_list() {
        let store = JsonFileRecentRepositoriesStore::new(temp_file_path());

        let recents = store.load().unwrap();

        assert!(recents.entries().is_empty());
    }

    #[test]
    fn save_then_load_round_trips_the_list() {
        let path = temp_file_path();
        let store = JsonFileRecentRepositoriesStore::new(path.clone());
        let mut recents = RecentRepositories::new();
        recents.touch(PathBuf::from("/repo-a"), 100);
        recents.touch(PathBuf::from("/repo-b"), 200);

        store.save(&recents).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded, recents);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_file_path().join("nested").join("recents.json");
        let store = JsonFileRecentRepositoriesStore::new(path.clone());
        let mut recents = RecentRepositories::new();
        recents.touch(PathBuf::from("/repo"), 1);

        store.save(&recents).unwrap();

        assert!(path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_corrupted_file_fails_with_parse_failure_rather_than_silently_returning_empty() {
        let path = temp_file_path();
        fs::write(&path, "{ not valid json").unwrap();
        let store = JsonFileRecentRepositoriesStore::new(path.clone());

        let err = store.load().unwrap_err();

        assert_eq!(err.code(), ErrorCode::ParseFailure);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_empty_file_is_treated_as_an_empty_list() {
        let path = temp_file_path();
        fs::write(&path, "").unwrap();
        let store = JsonFileRecentRepositoriesStore::new(path.clone());

        let recents = store.load().unwrap();

        assert!(recents.entries().is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_location_is_rooted_under_a_gitsail_desktop_directory() {
        let path = JsonFileRecentRepositoriesStore::default_location().unwrap();

        assert!(path.ends_with("gitsail/desktop/recent-repositories.json"));
    }
}
