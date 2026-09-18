//! Desktop update checking (T-260/US-127, EPIC-25 — Distribution &
//! Updates).
//!
//! ## What this is, given ADR-023
//!
//! ADR-023 (`docs/architecture/release-process.md`) records a real,
//! deliberate decision: GitSail distributes via GitHub Releases only, with
//! **no code signing/notarization** and **no signed auto-update channel**.
//! This module is therefore never an auto-installer: it only asks GitHub's
//! Releases API "what is the latest published release", compares its tag to
//! the running build's own tag, and reports the result — download and
//! installation stay a fully manual, explicit action by the person, exactly
//! like checking the releases page themselves. Claiming any stronger
//! "authenticity" guarantee (e.g. a verified signature) here would be
//! dishonest about a decision this project has already made; the integrity
//! story this module can honestly offer is "here is the exact GitHub
//! Release, here is its `SHA256SUMS.txt`, go verify it yourself" (US-127
//! criterion 1).
//!
//! ## Where this sits in Ports & Adapters
//!
//! [`UpdateCheckPort`] is the boundary — this module knows nothing about
//! HTTP or GitHub's JSON shape; `gitsail-forge`'s `GitHubReleaseUpdateAdapter`
//! implements it, mirroring `PullRequestQueryPort`/`GitHubPullRequestAdapter`
//! exactly (T-245's own precedent).
//!
//! ## Never an unsolicited network call (mandatory convention)
//! [`CheckForUpdate::execute`] with [`UpdateCheckTrigger::Automatic`] only
//! ever calls the port when both are true: the [`PreferencesPort`]-backed
//! "check for updates" toggle is on, and at least [`ONE_DAY_SECONDS`] have
//! passed since the last attempt (recorded in [`Preferences::
//! last_update_check_unix`] — persisted, so this throttle survives a
//! restart, never just an in-memory guard that resets on every launch).
//! [`UpdateCheckTrigger::Manual`] (an explicit "check for updates now" button
//! click) bypasses both — an explicit user action is, by definition, "some
//! form of user control" (this story's own wording), not an automatic call.
//! Either way, exactly one attempt happens per call; nothing in this module
//! loops or retries on its own.

use std::sync::Arc;

use gitsail_domain::{ErrorCode, GitSailError};

use crate::preferences::PreferencesPort;

/// One day, in seconds — the automatic-check throttle window (module-level
/// doc comment above). Not user-configurable in this first version; the
/// on/off toggle ([`Preferences::check_for_updates`]) is the control this
/// story asks for, a configurable interval is not.
pub const ONE_DAY_SECONDS: u64 = 24 * 60 * 60;

/// The data US-127 criterion 1 needs from GitHub's "latest release": enough
/// to show version/origin/integrity material without ever downloading
/// anything on the person's behalf.
///
/// `html_url`/`checksums_url`/`notes` are forge-authored content — the same
/// trust level `gitsail_application::pull_requests` already documents for
/// `PullRequestSummary`'s fields: never interpreted, only ever displayed as
/// inert text or opened as a plain link after this crate's caller
/// re-validates the host (see `apps/desktop/src-tauri/src/commands.rs`'s
/// `open_update_link`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// The release's own tag, e.g. `"v0.4.1"`.
    pub tag: String,
    /// The GitHub Release page — "origin" (US-127 criterion 1): opening
    /// this is how a person independently verifies where this information
    /// came from, the same way `release-process.md` documents for a manual
    /// check today.
    pub html_url: String,
    /// The `SHA256SUMS.txt` asset's own download URL, when the release
    /// carries one (every release `release.yml` has ever produced does;
    /// `None` only for a hand-crafted or pre-T-257 release, or a malformed
    /// double in a test).
    pub checksums_url: Option<String>,
    /// The release's own notes/changelog body, verbatim, or `None` when
    /// GitHub reports none.
    pub notes: Option<String>,
}

/// Every way asking GitHub for the latest release can fail, kept as narrow
/// as [`crate::pull_requests::PullRequestQueryError`]'s own precedent: only
/// the distinctions a caller must render differently.
#[derive(Debug)]
pub enum UpdateCheckError {
    /// Timeout, DNS failure, connection refused, offline, ... — a
    /// redacted, log-safe message (US-260 criterion 2: never a crash, never
    /// leaves the app looking broken).
    NetworkFailure(String),
    /// A response was received but could not be parsed/understood (a
    /// malformed body, an unrecognized status code) — DoD's "pacote
    /// inválido" case.
    Malformed(GitSailError),
    /// GitHub reported no releases exist yet for this repository (a bare
    /// `404` on `.../releases/latest`) — distinct from "malformed": this is
    /// a legitimate, if currently hypothetical, repository state.
    NoReleasesPublished,
}

/// Queries GitHub for the latest published release (US-127 criterion 1).
/// Implemented by `gitsail-forge`'s `GitHubReleaseUpdateAdapter`; see this
/// module's own top doc comment for why this crate never talks HTTP
/// directly.
pub trait UpdateCheckPort: Send + Sync {
    fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError>;
}

/// Distinguishes an explicit, user-initiated check from an automatic,
/// startup-triggered one — see this module's top doc comment for exactly
/// what differs between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckTrigger {
    Automatic,
    Manual,
}

/// Why [`CheckForUpdate::execute`] made no network call at all — always a
/// benign, expected state, never surfaced as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// [`Preferences::check_for_updates`] is off.
    Disabled,
    /// The last attempt (successful or not — see [`CheckForUpdate::execute`]'s
    /// own doc comment) was less than [`ONE_DAY_SECONDS`] ago.
    CheckedRecently { next_check_after_unix: u64 },
}

/// Every state a caller (the Desktop Tauri command, its own test) must be
/// able to render distinctly — deliberately not a single
/// `Result<ReleaseInfo, GitSailError>`: collapsing "no call was made",
/// "you're current", "an update exists", and "the check itself failed" into
/// one shape is exactly the ambiguity US-127's three acceptance criteria
/// exist to prevent (mirrors `ListPullRequestsOutcome`'s own precedent).
#[derive(Debug)]
pub enum UpdateCheckOutcome {
    /// No network call was made at all this time — see [`SkipReason`].
    Skipped(SkipReason),
    /// A call was made, GitHub reports no releases exist yet.
    NoReleasesPublished,
    /// A call was made; the running build is already the latest.
    UpToDate { current_tag: String },
    /// A call was made; GitHub's latest release is newer than the running
    /// build (US-127 criterion 1's whole point).
    UpdateAvailable {
        current_tag: String,
        release: ReleaseInfo,
    },
    /// A call was made and a newer release *might* exist, but the running
    /// build's own version could not be determined (a local/dev build not
    /// produced by `release.yml` — see `apps/desktop/src-tauri/src/
    /// version.rs`) or GitHub's own tag does not parse as `vMAJOR.MINOR.PATCH`
    /// so it cannot be safely compared. Honest rather than a guess in
    /// either direction: this is exactly the "cannot know" state, not
    /// silently reported as either up to date or available.
    CannotDetermineCurrentVersion { release: ReleaseInfo },
    /// The call itself failed (network or malformed response) — never
    /// panics or blocks the app; DoD's "falha ... nunca deixa a instalação
    /// inutilizável" for this feature's own simpler (check-only, no
    /// install) scope.
    CheckFailed { diagnostic: GitSailError },
}

/// Checks GitHub for a newer release than the one currently running (US-127
/// criterion 1), gated as this module's top doc comment describes.
pub struct CheckForUpdate {
    port: Arc<dyn UpdateCheckPort>,
    preferences: Arc<dyn PreferencesPort>,
}

impl CheckForUpdate {
    pub fn new(port: Arc<dyn UpdateCheckPort>, preferences: Arc<dyn PreferencesPort>) -> Self {
        Self { port, preferences }
    }

    /// `current_tag`: the running build's own release tag (e.g.
    /// `"v0.4.0"`), or `None` when it cannot be determined (a local/dev
    /// build — see [`UpdateCheckOutcome::CannotDetermineCurrentVersion`]).
    ///
    /// `now_unix`: the caller's current time, Unix seconds. Passed in
    /// explicitly rather than read from a `Clock` abstraction — this
    /// workspace's existing convention for a use case that needs "now"
    /// (e.g. `RecentRepositories::touch`'s own timestamp parameter), which
    /// keeps this deterministically testable with no time-mocking
    /// machinery.
    ///
    /// Every real attempt (whether it succeeds, fails, or finds "no
    /// releases yet") persists `now_unix` as [`Preferences::
    /// last_update_check_unix`] — including a [`UpdateCheckTrigger::Manual`]
    /// one, so an explicit "check now" also resets the automatic-check
    /// throttle window; it would be surprising for a person to click
    /// "check now" and then have an automatic check fire moments later. A
    /// failure to *persist* that timestamp is swallowed (best-effort only):
    /// it must never turn a check that actually ran into a failure the
    /// person sees, and worst case just means the throttle window did not
    /// advance this one time.
    pub fn execute(
        &self,
        trigger: UpdateCheckTrigger,
        current_tag: Option<&str>,
        now_unix: u64,
    ) -> UpdateCheckOutcome {
        let preferences = self
            .preferences
            .load()
            .map(|outcome| outcome.preferences)
            .unwrap_or_default();

        if trigger == UpdateCheckTrigger::Automatic {
            if !preferences.check_for_updates {
                return UpdateCheckOutcome::Skipped(SkipReason::Disabled);
            }
            if let Some(last_checked) = preferences.last_update_check_unix {
                let next_check_after_unix = last_checked.saturating_add(ONE_DAY_SECONDS);
                if now_unix < next_check_after_unix {
                    return UpdateCheckOutcome::Skipped(SkipReason::CheckedRecently {
                        next_check_after_unix,
                    });
                }
            }
        }

        let mut updated_preferences = preferences;
        updated_preferences.last_update_check_unix = Some(now_unix);
        let _ = self.preferences.save(&updated_preferences);

        match self.port.latest_release() {
            Ok(release) => Self::compare(current_tag, release),
            Err(UpdateCheckError::NetworkFailure(message)) => UpdateCheckOutcome::CheckFailed {
                diagnostic: GitSailError::new(
                    ErrorCode::NetworkFailure,
                    format!("could not check for updates: {message}"),
                )
                .with_remediation(
                    "check your network connection and try checking for updates again later",
                ),
            },
            Err(UpdateCheckError::Malformed(diagnostic)) => {
                UpdateCheckOutcome::CheckFailed { diagnostic }
            }
            Err(UpdateCheckError::NoReleasesPublished) => UpdateCheckOutcome::NoReleasesPublished,
        }
    }

    fn compare(current_tag: Option<&str>, release: ReleaseInfo) -> UpdateCheckOutcome {
        let Some(latest_version) = parse_semver_tag(&release.tag) else {
            return UpdateCheckOutcome::CheckFailed {
                diagnostic: GitSailError::new(
                    ErrorCode::ParseFailure,
                    format!(
                        "GitHub's latest release tag '{}' is not a recognizable vMAJOR.MINOR.PATCH version",
                        release.tag
                    ),
                ),
            };
        };
        let Some(current) = current_tag else {
            return UpdateCheckOutcome::CannotDetermineCurrentVersion { release };
        };
        let Some(current_version) = parse_semver_tag(current) else {
            return UpdateCheckOutcome::CannotDetermineCurrentVersion { release };
        };
        if latest_version > current_version {
            UpdateCheckOutcome::UpdateAvailable {
                current_tag: current.to_string(),
                release,
            }
        } else {
            UpdateCheckOutcome::UpToDate {
                current_tag: current.to_string(),
            }
        }
    }
}

/// Parses a `vMAJOR.MINOR.PATCH` release tag (ADR-023's own tagging
/// convention — see `.github/workflows/release.yml`'s `guard` job, which
/// only ever accepts `refs/tags/v*`) into a directly comparable tuple.
/// `None` for anything else, deliberately never a best-effort partial
/// parse: a tag this cannot confidently understand must never be silently
/// treated as "equal" or "older".
fn parse_semver_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let stripped = tag.strip_prefix('v')?;
    let mut parts = stripped.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{Preferences, PreferencesLoadOutcome};
    use std::sync::Mutex;

    struct InMemoryPreferences(Mutex<Preferences>);
    impl InMemoryPreferences {
        fn new(preferences: Preferences) -> Self {
            Self(Mutex::new(preferences))
        }
    }
    impl PreferencesPort for InMemoryPreferences {
        fn load(&self) -> Result<PreferencesLoadOutcome, GitSailError> {
            Ok(PreferencesLoadOutcome::clean(
                self.0.lock().unwrap().clone(),
            ))
        }
        fn save(&self, preferences: &Preferences) -> Result<(), GitSailError> {
            *self.0.lock().unwrap() = preferences.clone();
            Ok(())
        }
    }

    struct ScriptedPort(Mutex<Option<Result<ReleaseInfo, UpdateCheckErrorKind>>>);
    enum UpdateCheckErrorKind {
        Network(String),
        Malformed,
        NoReleases,
    }
    impl ScriptedPort {
        fn new(result: Result<ReleaseInfo, UpdateCheckErrorKind>) -> Self {
            Self(Mutex::new(Some(result)))
        }
    }
    impl UpdateCheckPort for ScriptedPort {
        fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError> {
            match self
                .0
                .lock()
                .unwrap()
                .take()
                .expect("called more than once")
            {
                Ok(release) => Ok(release),
                Err(UpdateCheckErrorKind::Network(msg)) => {
                    Err(UpdateCheckError::NetworkFailure(msg))
                }
                Err(UpdateCheckErrorKind::Malformed) => Err(UpdateCheckError::Malformed(
                    GitSailError::new(ErrorCode::ParseFailure, "boom"),
                )),
                Err(UpdateCheckErrorKind::NoReleases) => Err(UpdateCheckError::NoReleasesPublished),
            }
        }
    }

    fn release(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag: tag.to_string(),
            html_url: format!("https://github.com/rpaggi/gitsail/releases/tag/{tag}"),
            checksums_url: Some(format!(
                "https://github.com/rpaggi/gitsail/releases/download/{tag}/SHA256SUMS.txt"
            )),
            notes: Some("release notes".to_string()),
        }
    }

    #[test]
    fn a_newer_latest_release_is_reported_as_update_available() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.5.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000) {
            UpdateCheckOutcome::UpdateAvailable {
                current_tag,
                release,
            } => {
                assert_eq!(current_tag, "v0.4.0");
                assert_eq!(release.tag, "v0.5.0");
                assert!(release.checksums_url.is_some());
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn the_same_or_an_older_latest_release_is_up_to_date() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.4.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000) {
            UpdateCheckOutcome::UpToDate { current_tag } => assert_eq!(current_tag, "v0.4.0"),
            other => panic!("expected UpToDate, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_current_version_cannot_be_compared_and_says_so_honestly() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.5.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, None, 1_000) {
            UpdateCheckOutcome::CannotDetermineCurrentVersion { release } => {
                assert_eq!(release.tag, "v0.5.0");
            }
            other => panic!("expected CannotDetermineCurrentVersion, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_latest_tag_fails_the_check_rather_than_guessing() {
        let port = Arc::new(ScriptedPort::new(Ok(release("not-a-version"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000) {
            UpdateCheckOutcome::CheckFailed { diagnostic } => {
                assert_eq!(diagnostic.code(), ErrorCode::ParseFailure);
            }
            other => panic!("expected CheckFailed, got {other:?}"),
        }
    }

    #[test]
    fn a_network_failure_never_panics_and_is_reported_as_check_failed() {
        let port = Arc::new(ScriptedPort::new(Err(UpdateCheckErrorKind::Network(
            "connection refused".to_string(),
        ))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000) {
            UpdateCheckOutcome::CheckFailed { diagnostic } => {
                assert_eq!(diagnostic.code(), ErrorCode::NetworkFailure);
            }
            other => panic!("expected CheckFailed, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_response_never_panics_and_is_reported_as_check_failed() {
        let port = Arc::new(ScriptedPort::new(Err(UpdateCheckErrorKind::Malformed)));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        match use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000) {
            UpdateCheckOutcome::CheckFailed { diagnostic } => {
                assert_eq!(diagnostic.code(), ErrorCode::ParseFailure);
            }
            other => panic!("expected CheckFailed, got {other:?}"),
        }
    }

    #[test]
    fn no_releases_published_yet_is_distinct_from_a_failure() {
        let port = Arc::new(ScriptedPort::new(Err(UpdateCheckErrorKind::NoReleases)));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences);

        assert!(matches!(
            use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000),
            UpdateCheckOutcome::NoReleasesPublished
        ));
    }

    #[test]
    fn an_automatic_check_is_skipped_when_the_preference_is_disabled() {
        struct PanicPort;
        impl UpdateCheckPort for PanicPort {
            fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError> {
                panic!("must never call the network when disabled");
            }
        }
        let preferences = Arc::new(InMemoryPreferences::new(Preferences {
            check_for_updates: false,
            ..Preferences::default()
        }));
        let use_case = CheckForUpdate::new(Arc::new(PanicPort), preferences);

        assert!(matches!(
            use_case.execute(UpdateCheckTrigger::Automatic, Some("v0.4.0"), 1_000),
            UpdateCheckOutcome::Skipped(SkipReason::Disabled)
        ));
    }

    #[test]
    fn a_manual_check_bypasses_the_disabled_preference() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.4.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences {
            check_for_updates: false,
            ..Preferences::default()
        }));
        let use_case = CheckForUpdate::new(port, preferences);

        assert!(matches!(
            use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 1_000),
            UpdateCheckOutcome::UpToDate { .. }
        ));
    }

    #[test]
    fn an_automatic_check_is_skipped_when_the_last_check_was_less_than_a_day_ago() {
        struct PanicPort;
        impl UpdateCheckPort for PanicPort {
            fn latest_release(&self) -> Result<ReleaseInfo, UpdateCheckError> {
                panic!("must never call the network before the throttle window elapses");
            }
        }
        let preferences = Arc::new(InMemoryPreferences::new(Preferences {
            check_for_updates: true,
            last_update_check_unix: Some(1_000),
            ..Preferences::default()
        }));
        let use_case = CheckForUpdate::new(Arc::new(PanicPort), preferences);

        match use_case.execute(UpdateCheckTrigger::Automatic, Some("v0.4.0"), 1_000 + 3_600) {
            UpdateCheckOutcome::Skipped(SkipReason::CheckedRecently {
                next_check_after_unix,
            }) => {
                assert_eq!(next_check_after_unix, 1_000 + ONE_DAY_SECONDS);
            }
            other => panic!("expected Skipped(CheckedRecently), got {other:?}"),
        }
    }

    #[test]
    fn an_automatic_check_proceeds_once_the_throttle_window_has_elapsed() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.4.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences {
            check_for_updates: true,
            last_update_check_unix: Some(1_000),
            ..Preferences::default()
        }));
        let use_case = CheckForUpdate::new(port, preferences.clone());

        let now = 1_000 + ONE_DAY_SECONDS + 1;
        assert!(matches!(
            use_case.execute(UpdateCheckTrigger::Automatic, Some("v0.4.0"), now),
            UpdateCheckOutcome::UpToDate { .. }
        ));
        assert_eq!(
            preferences.0.lock().unwrap().last_update_check_unix,
            Some(now),
            "a real attempt must advance the throttle window"
        );
    }

    #[test]
    fn every_real_attempt_persists_the_check_timestamp_including_a_manual_one() {
        let port = Arc::new(ScriptedPort::new(Ok(release("v0.4.0"))));
        let preferences = Arc::new(InMemoryPreferences::new(Preferences::default()));
        let use_case = CheckForUpdate::new(port, preferences.clone());

        use_case.execute(UpdateCheckTrigger::Manual, Some("v0.4.0"), 42);

        assert_eq!(
            preferences.0.lock().unwrap().last_update_check_unix,
            Some(42)
        );
    }
}
