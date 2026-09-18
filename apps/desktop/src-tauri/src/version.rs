//! The running Desktop build's own version identifier (T-260/US-127).
//!
//! ## The gap this closes (ADR-021)
//!
//! ADR-021 (`docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`) pins every
//! workspace crate's Cargo version — and `tauri.conf.json`'s own `version`
//! field — at `"0.0.0"` through every pre-1.0 milestone; the real version
//! identifier is the git tag `release.yml` publishes under, tracked
//! entirely outside Cargo (the same way `SCHEMA_VERSION` is already tracked
//! independently of any crate's Cargo version — see
//! `docs/architecture/protocol-compatibility.md`). That leaves a real,
//! previously-unsolved gap this story's own scope calls out: a running
//! Desktop process has no way to know which release it *is*, since neither
//! `Cargo.toml` nor `tauri.conf.json` ever says anything other than
//! `0.0.0`.
//!
//! **The fix**: `build.rs` embeds `RELEASE_TAG` (already exported at the
//! workflow level by `.github/workflows/release.yml`, and already used by
//! that same `build-desktop` job to stamp the *installer's* own display
//! version — see `release-process.md`'s "Artifact versioning" section) as
//! a compile-time environment variable, `GITSAIL_APP_VERSION`, via
//! `cargo:rustc-env`. [`RUNNING_VERSION_TAG`] reads it with `env!` — always
//! defined, because `build.rs` always emits it, falling back to the
//! literal `"dev"` for any build that did not happen inside the release
//! pipeline (a local `cargo tauri dev`/`tauri build`, or `cargo test`).
//!
//! This is deliberately a *build-time* constant, not a runtime file read or
//! network call: the version a binary reports about itself must be exactly
//! the version it was compiled as, unable to drift after the fact.

/// Always defined (see this module's own doc comment): either a real
/// `vX.Y.Z` release tag, or the literal `"dev"` for a build the release
/// pipeline did not produce. Test-only/internal — production code should
/// call [`running_version_tag`] instead, which turns `"dev"` into the
/// honest `None` every other layer of this feature already expects.
pub const RUNNING_VERSION_TAG: &str = env!("GITSAIL_APP_VERSION");

/// This build's own release tag, or `None` when it cannot be determined
/// (a local/dev build — see this module's top doc comment). Fed straight
/// into `gitsail_application::CheckForUpdate::execute`'s `current_tag`
/// parameter, whose `None` case is exactly this: "cannot compare, but
/// still show whatever the latest release is" (see
/// [`gitsail_application::UpdateCheckOutcome::CannotDetermineCurrentVersion`]).
pub fn running_version_tag() -> Option<&'static str> {
    if RUNNING_VERSION_TAG == "dev" {
        None
    } else {
        Some(RUNNING_VERSION_TAG)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_version_tag_is_none_for_a_dev_build_never_a_fabricated_version() {
        // This test always runs against a build produced by `cargo test`,
        // never by `release.yml` (`RELEASE_TAG` is never set in this
        // sandbox/CI job) — so `RUNNING_VERSION_TAG` is always `"dev"`
        // here, and this assertion is exercising this module's real,
        // always-true-in-this-context behavior, not a mocked one.
        assert_eq!(RUNNING_VERSION_TAG, "dev");
        assert_eq!(running_version_tag(), None);
    }
}
