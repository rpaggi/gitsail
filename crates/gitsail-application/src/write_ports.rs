//! Write-side port for repository mutation (SAD §9, §10, §20; ADR-009).
//!
//! Deliberately a separate trait from [`crate::ports::RepositoryReadPort`]:
//! a consumer that only holds a `RepositoryReadPort` must never gain
//! mutation capability just by virtue of depending on this crate (ADR-009).
//! `gitsail-git`'s `GitCliProvider` implements both traits, but application
//! code depends on whichever capability it actually needs.
//!
//! Per SAD §20, `stage`/`unstage` are Safe operations and `commit` is
//! Moderate; the full risk-description/revalidation framework (US-111) and
//! cross-operation mutation serialization (US-116) are separate, not-yet-
//! built epics (EPIC-22/EPIC-23) and are out of scope here. A single
//! `RepositoryWritePort` call already only ever runs one `git` process at a
//! time, so it cannot corrupt the index by itself; coordinating *across*
//! concurrent calls from multiple sessions is left to that future work.

use std::path::PathBuf;

use gitsail_domain::{CommitHash, FileDiff, GitSailError, Repository};

/// Mutation capability against a Git repository's index and history
/// (SAD §9's v0.2/v0.3 mutation use cases; ADR-009).
pub trait RepositoryWritePort {
    /// Stages exactly `paths` into the index, including paths whose
    /// working-tree entry was deleted (US-011 criterion 1).
    fn stage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError>;

    /// Unstages exactly `paths`, leaving the working tree untouched. Works
    /// before the first commit exists (US-011 criteria 2, 3).
    fn unstage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError>;

    /// Commits exactly the current index content with `message`. Never
    /// creates an empty commit implicitly; returns the new commit's hash on
    /// success (US-012).
    fn create_commit(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError>;

    /// Stages only the hunks carried by `selection`, each element being a
    /// [`FileDiff`] trimmed down to the hunks to stage (typically a subset
    /// of what [`crate::ports::RepositoryReadPort::diff`] returned for the
    /// unstaged diff). The working tree is never touched. Fails, rather
    /// than guessing, when a hunk no longer applies cleanly to the current
    /// index (US-013 criteria 1-3).
    fn stage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError>;

    /// Unstages only the hunks carried by `selection` (typically a subset
    /// of the staged diff), leaving the working tree untouched and the rest
    /// of the index intact.
    fn unstage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError>;
}
