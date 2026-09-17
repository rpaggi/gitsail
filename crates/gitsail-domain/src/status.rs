//! Working tree and index status (SAD §8).

use std::path::PathBuf;

use crate::ids::BranchName;
use crate::repository::HeadState;

/// The kind of change a path underwent relative to its previous state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
    Ignored,
}

/// Per-path status, mirroring Git's distinct index and worktree states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileStatusCode {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    UpdatedButUnmerged,
    Untracked,
    Ignored,
}

/// A single changed path in the working tree or index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileChange {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub change_type: ChangeType,
    pub index_status: FileStatusCode,
    pub worktree_status: FileStatusCode,
}

/// Combined working tree and index state for a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryStatus {
    pub branch: Option<BranchName>,
    pub head_state: HeadState,
    pub files: Vec<FileChange>,
}

impl RepositoryStatus {
    /// Whether there are no pending changes in the index or working tree.
    pub fn is_clean(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_without_files_is_clean() {
        let status = RepositoryStatus {
            branch: None,
            head_state: HeadState::Unborn,
            files: vec![],
        };
        assert!(status.is_clean());
    }

    #[test]
    fn status_with_files_is_not_clean() {
        let status = RepositoryStatus {
            branch: None,
            head_state: HeadState::Unborn,
            files: vec![FileChange {
                path: "a.txt".into(),
                previous_path: None,
                change_type: ChangeType::Untracked,
                index_status: FileStatusCode::Untracked,
                worktree_status: FileStatusCode::Untracked,
            }],
        };
        assert!(!status.is_clean());
    }
}
