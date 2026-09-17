//! Repository entity and HEAD state (SAD §8).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::ids::{BranchName, CommitHash};

/// Opaque, stable identity for a discovered repository.
///
/// Derived from the canonical root path so the same on-disk repository
/// yields the same identity across sessions, independent of how it was
/// opened (relative path, symlink, etc.).
///
/// # Cross-platform path handling (SAD §30, US-008)
///
/// [`Repository::root_path`]/[`Repository::worktree_path`] are always the
/// exact `PathBuf` Git reported — Unicode, spaces, Windows drive letters
/// and UNC paths all round-trip through them untouched, since they are
/// never assembled from string fragments with a hardcoded separator.
///
/// `RepositoryId`'s inner string, in contrast, is a display/hashing
/// identity, not a filesystem path: it is derived with
/// [`Path::to_string_lossy`], which substitutes the platform's non-UTF-8
/// bytes (a valid but rare case on Unix; Windows paths are UTF-16 and
/// always convert losslessly). That lossy step is confined to this
/// identity — reads/writes always go through `root_path`/`worktree_path`
/// instead — but it is a known limitation: two on-disk paths differing
/// only in non-UTF-8 bytes could compare equal here. A future
/// `gitsail-protocol` representation of non-UTF-8 paths (SAD §14) must
/// make that limitation explicit to the caller rather than repeat this
/// silent substitution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryId(String);

impl RepositoryId {
    pub fn from_canonical_root(root: &Path) -> Self {
        Self(root.to_string_lossy().into_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// HEAD's current state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HeadState {
    /// HEAD points at a branch tip.
    Attached { branch: BranchName },
    /// HEAD points directly at a commit.
    Detached { commit: CommitHash },
    /// No commits exist yet (freshly initialized repository).
    Unborn,
}

/// A discovered Git repository.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repository {
    pub id: RepositoryId,
    pub root_path: PathBuf,
    pub worktree_path: Option<PathBuf>,
    pub is_bare: bool,
    pub head_state: HeadState,
    pub current_branch: Option<BranchName>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_for_the_same_canonical_path() {
        let a = RepositoryId::from_canonical_root(Path::new("/repo"));
        let b = RepositoryId::from_canonical_root(Path::new("/repo"));
        assert_eq!(a, b);
    }

    #[test]
    fn identity_differs_for_different_paths() {
        let a = RepositoryId::from_canonical_root(Path::new("/repo-a"));
        let b = RepositoryId::from_canonical_root(Path::new("/repo-b"));
        assert_ne!(a, b);
    }
}
