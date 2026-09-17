//! Repository entity and HEAD state (SAD §8).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::ids::{BranchName, CommitHash};

/// Opaque, stable identity for a discovered repository.
///
/// Derived from the canonical root path so the same on-disk repository
/// yields the same identity across sessions, independent of how it was
/// opened (relative path, symlink, etc.).
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
