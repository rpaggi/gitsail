//! Desktop's disk persistence for GitSail's own local UI preferences
//! (T-247/US-105).
//!
//! **Persistence decision**: same convention as
//! `recent_repositories_store.rs` (EPIC-11) — a plain JSON file at
//! `<OS config dir>/gitsail/desktop/preferences.json` (e.g.
//! `~/.config/gitsail/desktop/preferences.json` on Linux), resolved via the
//! `dirs` crate. Reused for the same reasons: the data is tiny, human
//! readable for debugging, and needs no query capability beyond "load the
//! whole value, save the whole value".
//!
//! **Corrupted-file handling deliberately differs** from
//! `JsonFileRecentRepositoriesStore`: US-105 criterion 3 requires an
//! invalid/corrupted preferences file to fall back to safe defaults *while
//! still reporting the problem*, never fail the caller outright the way a
//! corrupted recent-repositories file does today (recent-repositories has
//! no such requirement, so it keeps its original stricter behavior
//! unchanged). Here, `load()` only ever returns `Err` for a genuine
//! environment-level I/O failure while reading a file that does exist
//! (e.g. permission denied) — never for content that fails to parse as the
//! expected shape (missing file, empty file, malformed JSON, or an
//! unrecognized `theme` value all fold into
//! [`gitsail_application::PreferencesLoadOutcome::recovered`] instead).
//! This never risks losing the user's saved intent silently: the
//! diagnostic travels back to the caller either way, for `AppState`/UI to
//! surface, it just never blocks the app from starting with a usable
//! (default) preference set.
//!
//! **Not yet wired into `AppState`/Tauri commands.** T-247/US-105's own
//! scope is the storage mechanism itself (port + this adapter + tests),
//! deliberately modeling `theme` only "to prove the mechanism" — the story
//! explicitly defers the theme feature (reading it, applying it to the UI,
//! and any command surface a frontend would call) to T-248/US-106. That
//! story is expected to construct a [`JsonFilePreferencesStore`] in
//! `lib.rs::run()` and hand it to `AppState`, exactly the way
//! `recent_repositories_store::JsonFileRecentRepositoriesStore` is wired in
//! today. Until then this module has no caller in this crate, hence the
//! blanket allow below rather than leaving real, tested code flagged as
//! dead.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use gitsail_application::{Preferences, PreferencesLoadOutcome, PreferencesPort, ThemePreference};
use gitsail_domain::{ErrorCode, GitSailError};
use serde::{Deserialize, Serialize};

/// This adapter's own on-disk representation of [`ThemePreference`] —
/// `gitsail-application` intentionally has no `serde` dependency (Ports &
/// Adapters: the JSON format is this adapter's concern, not the domain's),
/// so the mapping lives here, explicitly, in both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredTheme {
    System,
    Light,
    Dark,
}

impl From<ThemePreference> for StoredTheme {
    fn from(theme: ThemePreference) -> Self {
        match theme {
            ThemePreference::System => StoredTheme::System,
            ThemePreference::Light => StoredTheme::Light,
            ThemePreference::Dark => StoredTheme::Dark,
        }
    }
}

impl From<StoredTheme> for ThemePreference {
    fn from(theme: StoredTheme) -> Self {
        match theme {
            StoredTheme::System => ThemePreference::System,
            StoredTheme::Light => ThemePreference::Light,
            StoredTheme::Dark => ThemePreference::Dark,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredPreferences {
    #[serde(default)]
    theme: Option<StoredTheme>,
}

/// A [`PreferencesPort`] backed by a single JSON file.
///
/// Guarded by its own [`Mutex`], matching
/// `JsonFileRecentRepositoriesStore`'s own reasoning: a "load, mutate, save"
/// round trip through `gitsail_application::SetThemePreference` is
/// otherwise not atomic across two separate port calls.
pub struct JsonFilePreferencesStore {
    file_path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFilePreferencesStore {
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
        Ok(config_dir.join("gitsail").join("desktop").join("preferences.json"))
    }

    fn read_error(err: std::io::Error) -> GitSailError {
        GitSailError::new(ErrorCode::Internal, "failed to read the preferences file")
            .with_source(err)
    }

    fn write_error(err: std::io::Error) -> GitSailError {
        GitSailError::new(ErrorCode::Internal, "failed to write the preferences file")
            .with_source(err)
    }

    fn corrupted_diagnostic(err: serde_json::Error) -> GitSailError {
        GitSailError::new(
            ErrorCode::ParseFailure,
            "the preferences file is corrupted or has an unrecognized value; falling back to defaults",
        )
        .with_remediation(
            "GitSail reset your local preferences to their defaults. Reapply any personalization \
             you had set; the invalid file was left in place for inspection.",
        )
        .with_source(err)
    }
}

impl PreferencesPort for JsonFilePreferencesStore {
    fn load(&self) -> Result<PreferencesLoadOutcome, GitSailError> {
        let _guard = self.lock.lock().expect("preferences store mutex poisoned");

        if !self.file_path.exists() {
            return Ok(PreferencesLoadOutcome::clean(Preferences::default()));
        }
        let contents = fs::read_to_string(&self.file_path).map_err(Self::read_error)?;
        if contents.trim().is_empty() {
            return Ok(PreferencesLoadOutcome::clean(Preferences::default()));
        }
        match serde_json::from_str::<StoredPreferences>(&contents) {
            Ok(stored) => {
                let theme = stored.theme.map(ThemePreference::from).unwrap_or_default();
                Ok(PreferencesLoadOutcome::clean(Preferences { theme }))
            }
            Err(err) => Ok(PreferencesLoadOutcome::recovered(Self::corrupted_diagnostic(err))),
        }
    }

    fn save(&self, preferences: &Preferences) -> Result<(), GitSailError> {
        let _guard = self.lock.lock().expect("preferences store mutex poisoned");

        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(Self::write_error)?;
        }
        let stored = StoredPreferences { theme: Some(StoredTheme::from(preferences.theme)) };
        let json = serde_json::to_string_pretty(&stored).map_err(|err| {
            GitSailError::new(ErrorCode::Internal, "failed to serialize preferences")
                .with_source(err)
        })?;
        fs::write(&self.file_path, json).map_err(Self::write_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A fresh, unique file path under the OS temp directory, mirroring
    /// `recent_repositories_store`'s own test helper.
    fn temp_file_path() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("gitsail-preferences-test-{}-{id}.json", std::process::id()))
    }

    #[test]
    fn load_without_a_file_yet_returns_clean_defaults() {
        let store = JsonFilePreferencesStore::new(temp_file_path());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.preferences, Preferences::default());
        assert!(outcome.diagnostic.is_none());
    }

    #[test]
    fn save_then_load_round_trips_a_valid_file() {
        let path = temp_file_path();
        let store = JsonFilePreferencesStore::new(path.clone());
        let preferences = Preferences { theme: ThemePreference::Dark };

        store.save(&preferences).unwrap();
        let outcome = store.load().unwrap();

        assert_eq!(outcome.preferences, preferences);
        assert!(outcome.diagnostic.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_file_path().join("nested").join("preferences.json");
        let store = JsonFilePreferencesStore::new(path.clone());

        store.save(&Preferences { theme: ThemePreference::Light }).unwrap();

        assert!(path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_corrupted_file_falls_back_to_defaults_with_a_diagnostic_rather_than_failing() {
        let path = temp_file_path();
        fs::write(&path, "{ not valid json").unwrap();
        let store = JsonFilePreferencesStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.preferences, Preferences::default());
        let diagnostic = outcome.diagnostic.expect("a corrupted file must report a diagnostic");
        assert_eq!(diagnostic.code(), ErrorCode::ParseFailure);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_unrecognized_theme_value_falls_back_to_defaults_with_a_diagnostic() {
        let path = temp_file_path();
        fs::write(&path, r#"{ "theme": "neon" }"#).unwrap();
        let store = JsonFilePreferencesStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.preferences, Preferences::default());
        assert!(outcome.diagnostic.is_some());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_empty_file_is_treated_as_clean_defaults() {
        let path = temp_file_path();
        fs::write(&path, "").unwrap();
        let store = JsonFilePreferencesStore::new(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(outcome.preferences, Preferences::default());
        assert!(outcome.diagnostic.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_location_is_rooted_under_a_gitsail_desktop_directory() {
        let path = JsonFilePreferencesStore::default_location().unwrap();

        assert!(path.ends_with("gitsail/desktop/preferences.json"));
    }
}
