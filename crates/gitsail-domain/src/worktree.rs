//! Worktree entity (SAD §8; EPIC-18/T-220/US-095).

use std::path::PathBuf;

use crate::ids::{BranchName, CommitHash};

/// What `HEAD` points at in a given worktree — mirrors [`crate::repository::
/// HeadState`], but kept as a separate type since a worktree listing is
/// reported for potentially many worktrees at once, distinct from the single
/// [`crate::repository::Repository`] a session has open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeHead {
    Attached {
        branch: BranchName,
    },
    Detached {
        commit: CommitHash,
    },
    /// No commits exist yet in this worktree.
    Unborn,
}

/// A single Git worktree (US-095 criterion 1: "a listagem associa path,
/// branch/HEAD e estado disponível (locked, prunable, etc. se o Git
/// expuser) de cada worktree").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: WorktreeHead,
    /// Whether this is the repository's original (non-linked) worktree.
    pub is_main: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}
