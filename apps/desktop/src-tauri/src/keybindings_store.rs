//! Desktop's disk persistence for custom keyboard shortcut overrides
//! (T-249/US-107).
//!
//! **Architecture decision** (documented per the task's own "decide and
//! document" requirement): unlike `preferences_store.rs` (T-247/T-248),
//! this deliberately does **not** go through
//! `gitsail_application::PreferencesPort`, and `gitsail-application` gains
//! no new preference field for it. Keybindings are a Desktop-only
//! presentation concern: the configurable "actions" (`focus-search`,
//! `fetch`, `pull`, `push`, `commit`, ...) and the shape a binding takes
//! (a normalized `Mod+Shift+K`-style string built from DOM
//! `KeyboardEvent` fields — see `src/keybindings.ts`) only make sense for
//! a DOM-based frontend. A terminal UI (`gitsail-tui`) reads raw terminal
//! key codes and would need an entirely different action set and binding
//! grammar; forcing both into one cross-surface `Preferences` type would
//! either leak Desktop-specific shape into the domain crate or require an
//! awkward untyped blob there. This store therefore holds nothing but a
//! plain `action id -> binding string` map: the action registry itself
//! (id/label/default binding) lives entirely in the frontend, and this
//! adapter never interprets what an id or a binding string means — it only
//! persists whatever the frontend hands it, exactly like
//! `JsonFilePreferencesStore` does for `theme`, just without a Core port
//! in front of it.
//!
//! **Persistence** mirrors `preferences_store.rs`'s own choice: a plain
//! JSON file at `<OS config dir>/gitsail/desktop/keybindings.json`. A
//! missing, empty, or corrupted file is treated as "no overrides yet"
//! (every action falls back to its frontend-defined default) rather than
//! failing the caller — the same reasoning `JsonFilePreferencesStore`
//! documents for a corrupted `theme` value: losing a few remapped
//! shortcuts to a corrupted file is recoverable and should never block the
//! app from starting.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use gitsail_domain::{ErrorCode, GitSailError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredKeybindings {
    #[serde(default)]
    overrides: HashMap<String, String>,
}

/// A plain JSON-file-backed store for keybinding overrides.
///
/// Guarded by its own [`Mutex`], matching every other JSON-file adapter in
/// this crate: a "load, mutate one entry, save" round trip
/// ([`Self::set_override`]) is otherwise not atomic across two separate
/// file accesses.
pub struct JsonFileKeybindingsStore {
    file_path: PathBuf,
    lock: Mutex<()>,
}

impl JsonFileKeybindingsStore {
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
        Ok(config_dir.join("gitsail").join("desktop").join("keybindings.json"))
    }

    fn read_locked(&self) -> Result<HashMap<String, String>, GitSailError> {
        if !self.file_path.exists() {
            return Ok(HashMap::new());
        }
        let contents = fs::read_to_string(&self.file_path).map_err(|err| {
            GitSailError::new(ErrorCode::Internal, "failed to read the keybindings file")
                .with_source(err)
        })?;
        if contents.trim().is_empty() {
            return Ok(HashMap::new());
        }
        // A corrupted/unrecognized file falls back to "no overrides" rather
        // than failing the caller (see module doc) — every action simply
        // reverts to its frontend-defined default, recoverable exactly like
        // a corrupted `preferences.json` falls back to `Preferences::default`.
        Ok(serde_json::from_str::<StoredKeybindings>(&contents).unwrap_or_default().overrides)
    }

    fn write_locked(&self, overrides: &HashMap<String, String>) -> Result<(), GitSailError> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                GitSailError::new(ErrorCode::Internal, "failed to write the keybindings file")
                    .with_source(err)
            })?;
        }
        let stored = StoredKeybindings { overrides: overrides.clone() };
        let json = serde_json::to_string_pretty(&stored).map_err(|err| {
            GitSailError::new(ErrorCode::Internal, "failed to serialize keybindings").with_source(err)
        })?;
        fs::write(&self.file_path, json).map_err(|err| {
            GitSailError::new(ErrorCode::Internal, "failed to write the keybindings file")
                .with_source(err)
        })
    }

    /// Loads every currently-overridden action id -> binding pair. An action
    /// with no entry here uses its frontend-defined default.
    pub fn load(&self) -> Result<HashMap<String, String>, GitSailError> {
        let _guard = self.lock.lock().expect("keybindings store mutex poisoned");
        self.read_locked()
    }

    /// Sets (`Some`) or clears (`None`, "restore default" — US-107
    /// criterion 1) one action's override, returning the full resulting
    /// map. This module never validates `action_id` or `binding` — it has
    /// no notion of the action registry or of conflicts (US-107 criterion
    /// 2); both checks are the frontend's job, against the map this
    /// returns.
    pub fn set_override(
        &self,
        action_id: &str,
        binding: Option<&str>,
    ) -> Result<HashMap<String, String>, GitSailError> {
        let _guard = self.lock.lock().expect("keybindings store mutex poisoned");
        let mut overrides = self.read_locked()?;
        match binding {
            Some(binding) => {
                overrides.insert(action_id.to_string(), binding.to_string());
            }
            None => {
                overrides.remove(action_id);
            }
        }
        self.write_locked(&overrides)?;
        Ok(overrides)
    }

    /// Clears every override at once (US-107 criterion 1's "restaurar
    /// padrão", applied to the whole action list rather than one action).
    pub fn reset_all(&self) -> Result<(), GitSailError> {
        let _guard = self.lock.lock().expect("keybindings store mutex poisoned");
        self.write_locked(&HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_file_path() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("gitsail-keybindings-test-{}-{id}.json", std::process::id()))
    }

    #[test]
    fn load_without_a_file_yet_returns_an_empty_map() {
        let store = JsonFileKeybindingsStore::new(temp_file_path());

        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn set_override_then_load_round_trips_the_binding() {
        let path = temp_file_path();
        let store = JsonFileKeybindingsStore::new(path.clone());

        store.set_override("focus-search", Some("Mod+Shift+K")).unwrap();
        let overrides = store.load().unwrap();

        assert_eq!(overrides.get("focus-search").map(String::as_str), Some("Mod+Shift+K"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_override_with_none_clears_a_previously_set_binding() {
        let path = temp_file_path();
        let store = JsonFileKeybindingsStore::new(path.clone());
        store.set_override("focus-search", Some("Mod+Shift+K")).unwrap();

        store.set_override("focus-search", None).unwrap();

        assert!(!store.load().unwrap().contains_key("focus-search"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reset_all_clears_every_override() {
        let path = temp_file_path();
        let store = JsonFileKeybindingsStore::new(path.clone());
        store.set_override("focus-search", Some("Mod+K")).unwrap();
        store.set_override("fetch", Some("Mod+Shift+F")).unwrap();

        store.reset_all().unwrap();

        assert!(store.load().unwrap().is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_corrupted_file_is_treated_as_no_overrides_rather_than_failing() {
        let path = temp_file_path();
        fs::write(&path, "{ not valid json").unwrap();
        let store = JsonFileKeybindingsStore::new(path.clone());

        assert!(store.load().unwrap().is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_location_is_rooted_under_a_gitsail_desktop_directory() {
        let path = JsonFileKeybindingsStore::default_location().unwrap();

        assert!(path.ends_with("gitsail/desktop/keybindings.json"));
    }
}
