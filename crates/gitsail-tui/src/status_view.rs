//! Flattens [`RepositoryStatus`] into selectable entries that separate the
//! index from the working tree (US-046 criterion 1: "Status separa index/
//! worktree e abre o diff correto").
//!
//! A single [`FileChange`] can be simultaneously staged and dirty in the
//! working tree (e.g. a file staged, then edited again) — [`FileChange`]
//! itself already carries `index_status`/`worktree_status` separately, but
//! the TUI needs a flat, cursor-addressable list where such a file appears
//! as *two* independently selectable entries, each opening the diff for
//! its own scope.

use std::path::PathBuf;

use gitsail_domain::{ChangeType, FileStatusCode, RepositoryStatus};

/// Which side of a [`gitsail_domain::FileChange`] a [`StatusEntry`] refers
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffScope {
    Staged,
    Worktree,
}

/// One selectable row in the status panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub change_type: ChangeType,
    pub scope: DiffScope,
}

fn is_relevant(code: FileStatusCode, scope: DiffScope) -> bool {
    match scope {
        // The index side has nothing to show for a purely untracked file —
        // `Untracked`/`Ignored` never describe index content.
        DiffScope::Staged => !matches!(
            code,
            FileStatusCode::Unmodified | FileStatusCode::Untracked | FileStatusCode::Ignored
        ),
        // The working-tree side legitimately includes `Untracked` (a new,
        // unstaged file) — only `Unmodified`/`Ignored` mean "nothing here".
        DiffScope::Worktree => {
            !matches!(code, FileStatusCode::Unmodified | FileStatusCode::Ignored)
        }
    }
}

/// Builds the flat, ordered list of selectable status entries for `status`.
/// A file with both a staged and a worktree change yields one entry per
/// scope, each independently selectable (US-046 criterion 1).
pub fn build_status_entries(status: &RepositoryStatus) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    for change in &status.files {
        if is_relevant(change.index_status, DiffScope::Staged) {
            entries.push(StatusEntry {
                path: change.path.clone(),
                previous_path: change.previous_path.clone(),
                change_type: change.change_type,
                scope: DiffScope::Staged,
            });
        }
        if is_relevant(change.worktree_status, DiffScope::Worktree) {
            entries.push(StatusEntry {
                path: change.path.clone(),
                previous_path: change.previous_path.clone(),
                change_type: change.change_type,
                scope: DiffScope::Worktree,
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{BranchName, FileChange, HeadState};

    fn status_with(files: Vec<FileChange>) -> RepositoryStatus {
        RepositoryStatus {
            branch: Some(BranchName::new("main").unwrap()),
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            files,
        }
    }

    #[test]
    fn a_purely_unstaged_modification_yields_one_worktree_entry() {
        let status = status_with(vec![FileChange {
            path: "a.txt".into(),
            previous_path: None,
            change_type: ChangeType::Modified,
            index_status: FileStatusCode::Unmodified,
            worktree_status: FileStatusCode::Modified,
        }]);

        let entries = build_status_entries(&status);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].scope, DiffScope::Worktree);
    }

    #[test]
    fn an_untracked_file_yields_one_worktree_entry_and_no_staged_entry() {
        let status = status_with(vec![FileChange {
            path: "new.txt".into(),
            previous_path: None,
            change_type: ChangeType::Untracked,
            index_status: FileStatusCode::Untracked,
            worktree_status: FileStatusCode::Untracked,
        }]);

        let entries = build_status_entries(&status);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].scope, DiffScope::Worktree);
    }

    #[test]
    fn a_file_staged_and_then_edited_again_yields_two_independent_entries() {
        let status = status_with(vec![FileChange {
            path: "a.txt".into(),
            previous_path: None,
            change_type: ChangeType::Modified,
            index_status: FileStatusCode::Modified,
            worktree_status: FileStatusCode::Modified,
        }]);

        let entries = build_status_entries(&status);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.scope == DiffScope::Staged));
        assert!(entries.iter().any(|e| e.scope == DiffScope::Worktree));
    }

    #[test]
    fn a_clean_status_yields_no_entries() {
        let status = status_with(vec![]);
        assert!(build_status_entries(&status).is_empty());
    }
}
