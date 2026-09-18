//! GitSail's own local, app-scoped UI preferences (T-247/US-105).
//!
//! **Precedence model** (US-105 criterion 1: "precedência é defaults →
//! usuário → repositório, quando justificado"): this module ships exactly
//! two tiers — a hardcoded default ([`Preferences::default`]) and one
//! user-level override, persisted by whichever [`PreferencesPort`] adapter
//! a frontend supplies (Desktop's is `preferences_store.rs`). There is
//! deliberately no third, per-repository tier in v0.3: [`ThemePreference`],
//! the one field this module models so far, is a pure presentation choice
//! a person reasonably expects to look the same no matter which repository
//! window they have open — a theme that silently flipped when switching
//! repositories would be surprising, not helpful, and nothing else is
//! modeled yet that would need to differ per repository either. Should a
//! future preference genuinely need a per-repository override (e.g. a
//! default author identity some people override for exactly one shared
//! work repository), that preference's own field gains an explicit
//! repository-scoped layer then — this module does not pre-build unused
//! repository-scope plumbing today.
//!
//! **Isolation from Git** (US-105 criterion 2): nothing in this module ever
//! reads or writes `.git/config`, or any other file Git itself
//! reads — this mirrors `privacy.rs` (EPIC-22/T-225/US-114), which already
//! established the pattern of modeling a GitSail-only app preference as a
//! plain domain type with no I/O of its own, decoupled from anything Git
//! considers configuration. An adapter (e.g. Desktop's JSON file store)
//! decides *where* a [`Preferences`] value lives; it never becomes part of
//! the repository the person is working in.
//!
//! **Persistence format** is an adapter concern (US-105 criterion 3); see
//! `apps/desktop/src-tauri/src/preferences_store.rs` for Desktop's choice
//! and the corrupted-file recovery this module's [`PreferencesLoadOutcome`]
//! type exists to support: a caller must always get back a usable
//! [`Preferences`] value (safe defaults, at worst), plus an optional
//! [`GitSailError`] diagnostic describing why the persisted value could not
//! be honored — the failure is reported, never silently swallowed, and
//! never a crash.

use std::sync::Arc;

use gitsail_domain::GitSailError;

/// The one preference this module models to prove the mechanism end to end
/// (T-247 criterion: "modele pelo menos uma preferência real"). This is
/// storage only — no theme is actually *applied* to any UI by this crate;
/// a future story (T-248/US-106) reads [`Preferences::theme`] and renders
/// accordingly.
///
/// Deliberately no `serde` derive here: this crate stays free of any
/// serialization-format dependency (see its `Cargo.toml` — only
/// `gitsail-domain`), matching Ports & Adapters (AGENTS.md: "Domain code
/// must not depend on infrastructure ... code"). An adapter that persists
/// this as JSON (e.g. Desktop's `preferences_store.rs`) owns its own
/// on-disk representation and converts to/from this type explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreference {
    /// Follow the OS/editor host's own theme. The safe, opinion-free
    /// default: a fresh install never overrides anything the person has
    /// not explicitly chosen.
    #[default]
    System,
    Light,
    Dark,
}

/// GitSail's own local UI preferences (US-105). Every field here is
/// app-only state: never Git configuration, never written to
/// `.git/config` (criterion 2), and — as of v0.3 — never scoped per
/// repository (see this module's own doc for why).
///
/// `check_for_updates`/`last_update_check_unix` (T-260/US-127) follow the
/// same "app-only, never Git config" rule: whether/when GitSail last asked
/// GitHub for its latest release is purely local UI state, never written
/// anywhere Git itself reads. Not `#[derive(Default)]`: `check_for_updates`
/// must default to `true` (checking is on by default, but always
/// switchable off — see `gitsail_application::update_check`'s own doc
/// comment on why this is never an unsolicited network call), which a
/// derived `Default` (`bool::default() == false`) would get backwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub theme: ThemePreference,
    /// Whether an *automatic* update check may ever run (T-260/US-127).
    /// Always overridable — this is the "always possible to disable via a
    /// preference" control the automatic-network-call convention requires.
    /// Never gates a manual "check for updates now" action.
    pub check_for_updates: bool,
    /// Unix seconds of the last real update-check attempt (successful or
    /// not), or `None` before the first one ever runs. Used only to
    /// throttle *automatic* checks (`gitsail_application::update_check`'s
    /// `ONE_DAY_SECONDS` window) — a manual check always bypasses this, but
    /// still advances it.
    pub last_update_check_unix: Option<u64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            check_for_updates: true,
            last_update_check_unix: None,
        }
    }
}

/// The result of loading [`Preferences`] from storage: always a usable
/// value, plus an optional diagnostic when the persisted value could not
/// be honored as-is (US-105 criterion 3).
#[derive(Debug)]
pub struct PreferencesLoadOutcome {
    /// The preferences to actually use — either what was persisted, or
    /// [`Preferences::default`] when recovering from a problem.
    pub preferences: Preferences,
    /// `Some` exactly when [`Self::preferences`] is a fallback rather than
    /// what the adapter actually found persisted (e.g. a corrupted file) —
    /// a caller surfaces this rather than silently discarding the person's
    /// saved intent without a word.
    pub diagnostic: Option<GitSailError>,
}

impl PreferencesLoadOutcome {
    /// The persisted value was read and honored as-is; nothing to report.
    pub fn clean(preferences: Preferences) -> Self {
        Self {
            preferences,
            diagnostic: None,
        }
    }

    /// The persisted value could not be honored (missing/corrupted/
    /// unreadable content); falls back to [`Preferences::default`] and
    /// carries `diagnostic` along for the caller to surface.
    pub fn recovered(diagnostic: GitSailError) -> Self {
        Self {
            preferences: Preferences::default(),
            diagnostic: Some(diagnostic),
        }
    }
}

/// Persists a [`Preferences`] value across process restarts (US-105).
/// Adapters decide the concrete storage — Desktop uses a JSON file under
/// the OS config directory (see `apps/desktop/src-tauri`), matching
/// [`crate::RecentRepositoriesPort`]'s own established convention. Unlike
/// that port, a corrupted/invalid persisted value must never surface as an
/// `Err` from [`Self::load`] — see [`PreferencesLoadOutcome`] for why: this
/// port's contract is "always return something usable", reserving `Err`
/// for genuine environment-level failures (e.g. the config directory is
/// unreadable) that no safe fallback can paper over.
pub trait PreferencesPort: Send + Sync {
    fn load(&self) -> Result<PreferencesLoadOutcome, GitSailError>;
    fn save(&self, preferences: &Preferences) -> Result<(), GitSailError>;
}

/// Loads the current preferences (US-105).
pub struct LoadPreferences {
    port: Arc<dyn PreferencesPort>,
}

impl LoadPreferences {
    pub fn new(port: Arc<dyn PreferencesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self) -> Result<PreferencesLoadOutcome, GitSailError> {
        self.port.load()
    }
}

/// Persists a full [`Preferences`] value, overwriting whatever was there
/// before.
pub struct SavePreferences {
    port: Arc<dyn PreferencesPort>,
}

impl SavePreferences {
    pub fn new(port: Arc<dyn PreferencesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, preferences: &Preferences) -> Result<(), GitSailError> {
        self.port.save(preferences)
    }
}

/// Updates just [`Preferences::theme`], preserving every other field's
/// current value (a "load, mutate one field, save" round trip, mirroring
/// `RecordRecentRepository`'s own shape) — the use case a future
/// theme-switching UI (T-248/US-106) calls; this crate never applies the
/// theme itself.
pub struct SetThemePreference {
    port: Arc<dyn PreferencesPort>,
}

impl SetThemePreference {
    pub fn new(port: Arc<dyn PreferencesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, theme: ThemePreference) -> Result<Preferences, GitSailError> {
        let mut preferences = self.port.load()?.preferences;
        preferences.theme = theme;
        self.port.save(&preferences)?;
        Ok(preferences)
    }
}

/// Updates just [`Preferences::check_for_updates`] (T-260/US-127's
/// mandatory "always possible to disable via a preference" control),
/// preserving every other field — the same "load, mutate one field, save"
/// shape [`SetThemePreference`] already establishes.
pub struct SetCheckForUpdatesPreference {
    port: Arc<dyn PreferencesPort>,
}

impl SetCheckForUpdatesPreference {
    pub fn new(port: Arc<dyn PreferencesPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, check_for_updates: bool) -> Result<Preferences, GitSailError> {
        let mut preferences = self.port.load()?.preferences;
        preferences.check_for_updates = check_for_updates;
        self.port.save(&preferences)?;
        Ok(preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An in-memory [`PreferencesPort`] double (T-247's required "double em
    /// memória"): exercises the use cases above without any filesystem
    /// dependency. Starts out as if nothing had ever been saved (`None`),
    /// distinct from "a value equal to defaults was saved" — `load()`
    /// reports the same clean outcome for both, matching what a real
    /// adapter observes on a missing file.
    struct InMemoryPreferences(Mutex<Option<Preferences>>);

    impl InMemoryPreferences {
        fn new() -> Self {
            Self(Mutex::new(None))
        }
    }

    impl PreferencesPort for InMemoryPreferences {
        fn load(&self) -> Result<PreferencesLoadOutcome, GitSailError> {
            let stored = self.0.lock().unwrap().clone().unwrap_or_default();
            Ok(PreferencesLoadOutcome::clean(stored))
        }

        fn save(&self, preferences: &Preferences) -> Result<(), GitSailError> {
            *self.0.lock().unwrap() = Some(preferences.clone());
            Ok(())
        }
    }

    #[test]
    fn theme_preference_defaults_to_system_never_a_forced_light_or_dark_choice() {
        assert_eq!(ThemePreference::default(), ThemePreference::System);
        assert_eq!(Preferences::default().theme, ThemePreference::System);
    }

    #[test]
    fn check_for_updates_defaults_to_on_with_no_check_ever_recorded_yet() {
        let defaults = Preferences::default();
        assert!(
            defaults.check_for_updates,
            "checking is on by default, but always switchable off (T-260/US-127)"
        );
        assert_eq!(defaults.last_update_check_unix, None);
    }

    #[test]
    fn loading_with_nothing_ever_saved_returns_clean_defaults() {
        let port: Arc<dyn PreferencesPort> = Arc::new(InMemoryPreferences::new());

        let outcome = LoadPreferences::new(port).execute().unwrap();

        assert_eq!(outcome.preferences, Preferences::default());
        assert!(outcome.diagnostic.is_none());
    }

    #[test]
    fn save_then_load_round_trips_the_preferences() {
        let port: Arc<dyn PreferencesPort> = Arc::new(InMemoryPreferences::new());
        let saved = Preferences {
            theme: ThemePreference::Dark,
            ..Preferences::default()
        };

        SavePreferences::new(port.clone()).execute(&saved).unwrap();
        let loaded = LoadPreferences::new(port).execute().unwrap();

        assert_eq!(loaded.preferences, saved);
        assert!(loaded.diagnostic.is_none());
    }

    #[test]
    fn set_theme_preference_persists_only_the_theme_field() {
        let port: Arc<dyn PreferencesPort> = Arc::new(InMemoryPreferences::new());

        let updated = SetThemePreference::new(port.clone())
            .execute(ThemePreference::Light)
            .unwrap();

        assert_eq!(updated.theme, ThemePreference::Light);
        assert_eq!(
            LoadPreferences::new(port)
                .execute()
                .unwrap()
                .preferences
                .theme,
            ThemePreference::Light
        );
    }

    #[test]
    fn set_check_for_updates_preference_persists_only_that_field() {
        let port: Arc<dyn PreferencesPort> = Arc::new(InMemoryPreferences::new());
        SetThemePreference::new(port.clone())
            .execute(ThemePreference::Dark)
            .unwrap();

        let updated = SetCheckForUpdatesPreference::new(port.clone())
            .execute(false)
            .unwrap();

        assert!(!updated.check_for_updates);
        assert_eq!(
            updated.theme,
            ThemePreference::Dark,
            "toggling the update-check preference must never disturb the theme"
        );
        assert!(
            !LoadPreferences::new(port)
                .execute()
                .unwrap()
                .preferences
                .check_for_updates
        );
    }

    #[test]
    fn a_recovered_outcome_always_carries_safe_defaults_and_a_diagnostic() {
        let diagnostic = GitSailError::new(gitsail_domain::ErrorCode::ParseFailure, "corrupted");

        let outcome = PreferencesLoadOutcome::recovered(diagnostic);

        assert_eq!(outcome.preferences, Preferences::default());
        assert!(outcome.diagnostic.is_some());
        assert_eq!(
            outcome.diagnostic.unwrap().code(),
            gitsail_domain::ErrorCode::ParseFailure
        );
    }
}
