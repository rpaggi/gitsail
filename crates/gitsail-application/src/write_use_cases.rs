//! Mutation use cases (SAD §9's v0.2/v0.3 list). Each depends on
//! [`RepositoryWritePort`] rather than a concrete adapter, so it can be
//! exercised with a test double (ADR-002, ADR-009), mirroring
//! `use_cases.rs`'s read-side pattern.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitsail_domain::{BranchName, CommitHash, FileDiff, GitSailError, Repository, Stash, Worktree};

use crate::write_ports::{
    RepositoryWritePort, StashApplyOutcome, StashScope, TagAnnotation, WorktreeBranchSpec,
};

pub struct StageFiles {
    port: Arc<dyn RepositoryWritePort>,
}

impl StageFiles {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        self.port.stage_files(repo, paths)
    }
}

pub struct UnstageFiles {
    port: Arc<dyn RepositoryWritePort>,
}

impl UnstageFiles {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        self.port.unstage_files(repo, paths)
    }
}

pub struct CreateCommit {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateCommit {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError> {
        self.port.create_commit(repo, message)
    }
}

pub struct StageHunks {
    port: Arc<dyn RepositoryWritePort>,
}

impl StageHunks {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        self.port.stage_hunks(repo, selection)
    }
}

pub struct UnstageHunks {
    port: Arc<dyn RepositoryWritePort>,
}

impl UnstageHunks {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        self.port.unstage_hunks(repo, selection)
    }
}

pub struct SwitchBranch {
    port: Arc<dyn RepositoryWritePort>,
}

impl SwitchBranch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, target: &BranchName) -> Result<(), GitSailError> {
        self.port.switch_branch(repo, target)
    }
}

pub struct CreateBranch {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateBranch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        name: &BranchName,
        start_point: Option<&CommitHash>,
    ) -> Result<(), GitSailError> {
        self.port.create_branch(repo, name, start_point)
    }
}

pub struct DeleteBranch {
    port: Arc<dyn RepositoryWritePort>,
}

impl DeleteBranch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, name: &BranchName, force: bool) -> Result<(), GitSailError> {
        self.port.delete_branch(repo, name, force)
    }
}

/// Amends `HEAD` (US-059). See [`RepositoryWritePort::amend_commit`] for the
/// `expected_head` revalidation contract this delegates to unchanged.
pub struct AmendCommit {
    port: Arc<dyn RepositoryWritePort>,
}

impl AmendCommit {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        message: &str,
        expected_head: &CommitHash,
    ) -> Result<CommitHash, GitSailError> {
        self.port.amend_commit(repo, message, expected_head)
    }
}

/// Creates a new stash (US-092). See [`RepositoryWritePort::create_stash`]
/// for the scope/message contract this delegates to unchanged.
pub struct CreateStash {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateStash {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        message: Option<&str>,
        scope: StashScope,
    ) -> Result<Stash, GitSailError> {
        self.port.create_stash(repo, message, scope)
    }
}

/// Applies a stash without removing it (US-093). See
/// [`RepositoryWritePort::apply_stash`] for the revalidation/conflict
/// contract this delegates to unchanged.
pub struct ApplyStash {
    port: Arc<dyn RepositoryWritePort>,
}

impl ApplyStash {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        self.port.apply_stash(repo, expected)
    }
}

/// Applies a stash and removes it, unless applying produced conflicts
/// (US-093). See [`RepositoryWritePort::pop_stash`].
pub struct PopStash {
    port: Arc<dyn RepositoryWritePort>,
}

impl PopStash {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        self.port.pop_stash(repo, expected)
    }
}

/// Deletes a stash without applying it (US-093; `Destructive`, see
/// [`crate::mutation::MutationKind::DropStash`]). See
/// [`RepositoryWritePort::drop_stash`].
pub struct DropStash {
    port: Arc<dyn RepositoryWritePort>,
}

impl DropStash {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, expected: &Stash) -> Result<(), GitSailError> {
        self.port.drop_stash(repo, expected)
    }
}

/// Creates a local tag (US-094). See [`RepositoryWritePort::create_tag`].
pub struct CreateTag {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateTag {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        name: &str,
        target: Option<&CommitHash>,
        annotation: TagAnnotation,
    ) -> Result<(), GitSailError> {
        self.port.create_tag(repo, name, target, annotation)
    }
}

/// Deletes a local tag (US-094). See [`RepositoryWritePort::delete_tag`].
pub struct DeleteTag {
    port: Arc<dyn RepositoryWritePort>,
}

impl DeleteTag {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, name: &str) -> Result<(), GitSailError> {
        self.port.delete_tag(repo, name)
    }
}

/// Creates a new worktree (US-095). See
/// [`RepositoryWritePort::create_worktree`].
pub struct CreateWorktree {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateWorktree {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        path: &Path,
        branch: WorktreeBranchSpec,
    ) -> Result<Worktree, GitSailError> {
        self.port.create_worktree(repo, path, branch)
    }
}

/// Removes a worktree (US-095). See
/// [`RepositoryWritePort::remove_worktree`].
pub struct RemoveWorktree {
    port: Arc<dyn RepositoryWritePort>,
}

impl RemoveWorktree {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, path: &Path, force: bool) -> Result<(), GitSailError> {
        self.port.remove_worktree(repo, path, force)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        BranchName, ErrorCode, GitTimestamp, HeadState, RepositoryId, WorktreeHead,
    };
    use std::path::Path;
    use std::sync::Mutex;

    /// A double standing in for a real Git adapter (e.g. `gitsail-git`),
    /// exercised with no shell/process/Git dependency, demonstrating that
    /// these use cases can be exercised against any `RepositoryWritePort`
    /// implementation (mirroring `use_cases.rs`'s `FakeReadPort`).
    struct FakeWritePort {
        commit_hash: CommitHash,
        fail: bool,
        received_stage: Mutex<Option<Vec<PathBuf>>>,
        received_unstage: Mutex<Option<Vec<PathBuf>>>,
        received_commit_message: Mutex<Option<String>>,
        received_stage_hunks: Mutex<Option<Vec<FileDiff>>>,
        received_unstage_hunks: Mutex<Option<Vec<FileDiff>>>,
        received_switch_target: Mutex<Option<BranchName>>,
        received_create_branch: Mutex<Option<(BranchName, Option<CommitHash>)>>,
        received_delete_branch: Mutex<Option<(BranchName, bool)>>,
        received_amend: Mutex<Option<(String, CommitHash)>>,
        amended_hash: CommitHash,
        received_create_stash: Mutex<Option<(Option<String>, StashScope)>>,
        created_stash: Stash,
        received_apply_stash: Mutex<Option<Stash>>,
        received_pop_stash: Mutex<Option<Stash>>,
        received_drop_stash: Mutex<Option<Stash>>,
        apply_outcome: StashApplyOutcome,
        received_create_tag: Mutex<Option<(String, Option<CommitHash>, TagAnnotation)>>,
        received_delete_tag: Mutex<Option<String>>,
        received_create_worktree: Mutex<Option<(PathBuf, WorktreeBranchSpec)>>,
        created_worktree: Worktree,
        received_remove_worktree: Mutex<Option<(PathBuf, bool)>>,
    }

    fn sample_stash() -> Stash {
        Stash {
            index: 0,
            commit: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            message: "WIP on main: original".to_string(),
            date: GitTimestamp::new(0, 0),
        }
    }

    fn sample_worktree() -> Worktree {
        Worktree {
            path: PathBuf::from("/repo-wt"),
            head: WorktreeHead::Attached {
                branch: BranchName::new("feature").unwrap(),
            },
            is_main: false,
            is_locked: false,
            is_prunable: false,
        }
    }

    impl FakeWritePort {
        fn new() -> Self {
            Self {
                commit_hash: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
                fail: false,
                received_stage: Mutex::new(None),
                received_unstage: Mutex::new(None),
                received_commit_message: Mutex::new(None),
                received_stage_hunks: Mutex::new(None),
                received_unstage_hunks: Mutex::new(None),
                received_switch_target: Mutex::new(None),
                received_create_branch: Mutex::new(None),
                received_delete_branch: Mutex::new(None),
                received_amend: Mutex::new(None),
                amended_hash: CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap(),
                received_create_stash: Mutex::new(None),
                created_stash: sample_stash(),
                received_apply_stash: Mutex::new(None),
                received_pop_stash: Mutex::new(None),
                received_drop_stash: Mutex::new(None),
                apply_outcome: StashApplyOutcome {
                    had_conflicts: false,
                },
                received_create_tag: Mutex::new(None),
                received_delete_tag: Mutex::new(None),
                received_create_worktree: Mutex::new(None),
                created_worktree: sample_worktree(),
                received_remove_worktree: Mutex::new(None),
            }
        }

        fn failing() -> Self {
            Self {
                fail: true,
                ..Self::new()
            }
        }
    }

    impl RepositoryWritePort for FakeWritePort {
        fn stage_files(&self, _repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
            *self.received_stage.lock().unwrap() = Some(paths.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale status"));
            }
            Ok(())
        }

        fn unstage_files(&self, _repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
            *self.received_unstage.lock().unwrap() = Some(paths.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale status"));
            }
            Ok(())
        }

        fn create_commit(
            &self,
            _repo: &Repository,
            message: &str,
        ) -> Result<CommitHash, GitSailError> {
            *self.received_commit_message.lock().unwrap() = Some(message.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "nothing staged to commit",
                ));
            }
            Ok(self.commit_hash.clone())
        }

        fn stage_hunks(
            &self,
            _repo: &Repository,
            selection: &[FileDiff],
        ) -> Result<(), GitSailError> {
            *self.received_stage_hunks.lock().unwrap() = Some(selection.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale diff"));
            }
            Ok(())
        }

        fn unstage_hunks(
            &self,
            _repo: &Repository,
            selection: &[FileDiff],
        ) -> Result<(), GitSailError> {
            *self.received_unstage_hunks.lock().unwrap() = Some(selection.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale diff"));
            }
            Ok(())
        }

        fn switch_branch(&self, _repo: &Repository, target: &BranchName) -> Result<(), GitSailError> {
            *self.received_switch_target.lock().unwrap() = Some(target.clone());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "would overwrite local changes",
                ));
            }
            Ok(())
        }

        fn create_branch(
            &self,
            _repo: &Repository,
            name: &BranchName,
            start_point: Option<&CommitHash>,
        ) -> Result<(), GitSailError> {
            *self.received_create_branch.lock().unwrap() =
                Some((name.clone(), start_point.cloned()));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "already exists",
                ));
            }
            Ok(())
        }

        fn delete_branch(
            &self,
            _repo: &Repository,
            name: &BranchName,
            force: bool,
        ) -> Result<(), GitSailError> {
            *self.received_delete_branch.lock().unwrap() = Some((name.clone(), force));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "not fully merged",
                ));
            }
            Ok(())
        }

        fn amend_commit(
            &self,
            _repo: &Repository,
            message: &str,
            expected_head: &CommitHash,
        ) -> Result<CommitHash, GitSailError> {
            *self.received_amend.lock().unwrap() =
                Some((message.to_string(), expected_head.clone()));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "HEAD changed since the amend was previewed",
                ));
            }
            Ok(self.amended_hash.clone())
        }

        fn create_stash(
            &self,
            _repo: &Repository,
            message: Option<&str>,
            scope: StashScope,
        ) -> Result<Stash, GitSailError> {
            *self.received_create_stash.lock().unwrap() = Some((message.map(str::to_string), scope));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "nothing to stash",
                ));
            }
            Ok(self.created_stash.clone())
        }

        fn apply_stash(
            &self,
            _repo: &Repository,
            expected: &Stash,
        ) -> Result<StashApplyOutcome, GitSailError> {
            *self.received_apply_stash.lock().unwrap() = Some(expected.clone());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "stash list changed since it was previewed",
                ));
            }
            Ok(self.apply_outcome)
        }

        fn pop_stash(
            &self,
            _repo: &Repository,
            expected: &Stash,
        ) -> Result<StashApplyOutcome, GitSailError> {
            *self.received_pop_stash.lock().unwrap() = Some(expected.clone());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "stash list changed since it was previewed",
                ));
            }
            Ok(self.apply_outcome)
        }

        fn drop_stash(&self, _repo: &Repository, expected: &Stash) -> Result<(), GitSailError> {
            *self.received_drop_stash.lock().unwrap() = Some(expected.clone());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "stash list changed since it was previewed",
                ));
            }
            Ok(())
        }

        fn create_tag(
            &self,
            _repo: &Repository,
            name: &str,
            target: Option<&CommitHash>,
            annotation: TagAnnotation,
        ) -> Result<(), GitSailError> {
            *self.received_create_tag.lock().unwrap() =
                Some((name.to_string(), target.cloned(), annotation));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "a tag with that name already exists",
                ));
            }
            Ok(())
        }

        fn delete_tag(&self, _repo: &Repository, name: &str) -> Result<(), GitSailError> {
            *self.received_delete_tag.lock().unwrap() = Some(name.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::RepositoryNotFound, "no such tag"));
            }
            Ok(())
        }

        fn create_worktree(
            &self,
            _repo: &Repository,
            path: &Path,
            branch: WorktreeBranchSpec,
        ) -> Result<Worktree, GitSailError> {
            *self.received_create_worktree.lock().unwrap() = Some((path.to_path_buf(), branch));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "branch already checked out elsewhere",
                ));
            }
            Ok(self.created_worktree.clone())
        }

        fn remove_worktree(
            &self,
            _repo: &Repository,
            path: &Path,
            force: bool,
        ) -> Result<(), GitSailError> {
            *self.received_remove_worktree.lock().unwrap() = Some((path.to_path_buf(), force));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "worktree has uncommitted changes",
                ));
            }
            Ok(())
        }
    }

    fn sample_repository() -> Repository {
        Repository {
            id: RepositoryId::from_canonical_root(Path::new("/repo")),
            root_path: PathBuf::from("/repo"),
            worktree_path: Some(PathBuf::from("/repo")),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        }
    }

    fn sample_file_diff() -> FileDiff {
        FileDiff {
            path: PathBuf::from("a.txt"),
            previous_path: None,
            change_type: gitsail_domain::ChangeType::Modified,
            is_binary: false,
            truncated: false,
            hunks: vec![],
        }
    }

    #[test]
    fn stage_files_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = StageFiles::new(port.clone());
        let paths = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];

        use_case.execute(&sample_repository(), &paths).unwrap();

        assert_eq!(*port.received_stage.lock().unwrap(), Some(paths));
    }

    #[test]
    fn stage_files_propagates_port_error() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = StageFiles::new(port);

        let err = use_case
            .execute(&sample_repository(), &[PathBuf::from("a.txt")])
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn unstage_files_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = UnstageFiles::new(port.clone());
        let paths = vec![PathBuf::from("a.txt")];

        use_case.execute(&sample_repository(), &paths).unwrap();

        assert_eq!(*port.received_unstage.lock().unwrap(), Some(paths));
    }

    #[test]
    fn create_commit_returns_hash_on_success() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateCommit::new(port.clone());

        let hash = use_case.execute(&sample_repository(), "a message").unwrap();

        assert_eq!(hash, port.commit_hash);
        assert_eq!(
            *port.received_commit_message.lock().unwrap(),
            Some("a message".to_string())
        );
    }

    #[test]
    fn create_commit_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateCommit::new(port);

        let err = use_case
            .execute(&sample_repository(), "a message")
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn stage_hunks_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = StageHunks::new(port.clone());
        let selection = vec![sample_file_diff()];

        use_case.execute(&sample_repository(), &selection).unwrap();

        assert_eq!(*port.received_stage_hunks.lock().unwrap(), Some(selection));
    }

    #[test]
    fn unstage_hunks_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = UnstageHunks::new(port.clone());
        let selection = vec![sample_file_diff()];

        use_case
            .execute(&sample_repository(), &selection)
            .unwrap();

        assert_eq!(
            *port.received_unstage_hunks.lock().unwrap(),
            Some(selection)
        );
    }

    #[test]
    fn switch_branch_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = SwitchBranch::new(port.clone());
        let target = BranchName::new("feature").unwrap();

        use_case.execute(&sample_repository(), &target).unwrap();

        assert_eq!(*port.received_switch_target.lock().unwrap(), Some(target));
    }

    #[test]
    fn switch_branch_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = SwitchBranch::new(port);

        let err = use_case
            .execute(&sample_repository(), &BranchName::new("feature").unwrap())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn create_branch_delegates_to_port_with_start_point() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateBranch::new(port.clone());
        let name = BranchName::new("feature").unwrap();
        let start_point = CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();

        use_case
            .execute(&sample_repository(), &name, Some(&start_point))
            .unwrap();

        assert_eq!(
            *port.received_create_branch.lock().unwrap(),
            Some((name, Some(start_point)))
        );
    }

    #[test]
    fn create_branch_propagates_port_error_on_name_collision() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateBranch::new(port);

        let err = use_case
            .execute(&sample_repository(), &BranchName::new("main").unwrap(), None)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn delete_branch_delegates_to_port_with_force_flag() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = DeleteBranch::new(port.clone());
        let name = BranchName::new("feature").unwrap();

        use_case.execute(&sample_repository(), &name, true).unwrap();

        assert_eq!(
            *port.received_delete_branch.lock().unwrap(),
            Some((name, true))
        );
    }

    #[test]
    fn delete_branch_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = DeleteBranch::new(port);

        let err = use_case
            .execute(&sample_repository(), &BranchName::new("feature").unwrap(), false)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn amend_commit_delegates_to_port_with_message_and_expected_head() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = AmendCommit::new(port.clone());
        let expected_head =
            CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();

        let hash = use_case
            .execute(&sample_repository(), "amended message", &expected_head)
            .unwrap();

        assert_eq!(hash, port.amended_hash);
        assert_eq!(
            *port.received_amend.lock().unwrap(),
            Some(("amended message".to_string(), expected_head))
        );
    }

    #[test]
    fn amend_commit_propagates_a_conflict_when_head_moved_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = AmendCommit::new(port);
        let expected_head =
            CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();

        let err = use_case
            .execute(&sample_repository(), "amended message", &expected_head)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -----------------------------------------------------------------
    // EPIC-18: stash, tag, worktree use cases.
    // -----------------------------------------------------------------

    #[test]
    fn create_stash_delegates_to_port_with_message_and_scope() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateStash::new(port.clone());
        let scope = StashScope {
            keep_index: true,
            include_untracked: true,
            all: false,
        };

        let stash = use_case
            .execute(&sample_repository(), Some("WIP: refactor"), scope)
            .unwrap();

        assert_eq!(stash, port.created_stash);
        assert_eq!(
            *port.received_create_stash.lock().unwrap(),
            Some((Some("WIP: refactor".to_string()), scope))
        );
    }

    #[test]
    fn create_stash_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateStash::new(port);

        let err = use_case
            .execute(&sample_repository(), None, StashScope::default())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn apply_stash_delegates_to_port_with_the_previewed_entry() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = ApplyStash::new(port.clone());
        let expected = sample_stash();

        let outcome = use_case.execute(&sample_repository(), &expected).unwrap();

        assert!(!outcome.had_conflicts);
        assert_eq!(*port.received_apply_stash.lock().unwrap(), Some(expected));
    }

    #[test]
    fn apply_stash_propagates_a_stale_identity_conflict_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = ApplyStash::new(port);

        let err = use_case
            .execute(&sample_repository(), &sample_stash())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn pop_stash_delegates_to_port_and_reports_conflicts_distinctly() {
        let mut port = FakeWritePort::new();
        port.apply_outcome = StashApplyOutcome { had_conflicts: true };
        let port = Arc::new(port);
        let use_case = PopStash::new(port.clone());
        let expected = sample_stash();

        let outcome = use_case.execute(&sample_repository(), &expected).unwrap();

        assert!(
            outcome.had_conflicts,
            "a conflicted pop must never be reported as a plain success"
        );
        assert_eq!(*port.received_pop_stash.lock().unwrap(), Some(expected));
    }

    #[test]
    fn drop_stash_delegates_to_port_with_the_previewed_entry() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = DropStash::new(port.clone());
        let expected = sample_stash();

        use_case.execute(&sample_repository(), &expected).unwrap();

        assert_eq!(*port.received_drop_stash.lock().unwrap(), Some(expected));
    }

    #[test]
    fn drop_stash_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = DropStash::new(port);

        let err = use_case
            .execute(&sample_repository(), &sample_stash())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn create_tag_delegates_to_port_with_name_target_and_annotation() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateTag::new(port.clone());
        let target = CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let annotation = TagAnnotation::Annotated {
            message: "release".to_string(),
        };

        use_case
            .execute(&sample_repository(), "v1.0", Some(&target), annotation.clone())
            .unwrap();

        assert_eq!(
            *port.received_create_tag.lock().unwrap(),
            Some(("v1.0".to_string(), Some(target), annotation))
        );
    }

    #[test]
    fn create_tag_propagates_a_name_collision_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateTag::new(port);

        let err = use_case
            .execute(&sample_repository(), "v1.0", None, TagAnnotation::Lightweight)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn delete_tag_delegates_to_port_with_the_exact_name() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = DeleteTag::new(port.clone());

        use_case.execute(&sample_repository(), "v1.0").unwrap();

        assert_eq!(
            *port.received_delete_tag.lock().unwrap(),
            Some("v1.0".to_string())
        );
    }

    #[test]
    fn delete_tag_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = DeleteTag::new(port);

        let err = use_case.execute(&sample_repository(), "v1.0").unwrap_err();

        assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
    }

    #[test]
    fn create_worktree_delegates_to_port_with_path_and_branch_spec() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateWorktree::new(port.clone());
        let branch = WorktreeBranchSpec::NewBranch {
            name: BranchName::new("feature").unwrap(),
            start_point: None,
        };

        let worktree = use_case
            .execute(&sample_repository(), Path::new("/repo-wt"), branch.clone())
            .unwrap();

        assert_eq!(worktree, port.created_worktree);
        assert_eq!(
            *port.received_create_worktree.lock().unwrap(),
            Some((PathBuf::from("/repo-wt"), branch))
        );
    }

    #[test]
    fn create_worktree_propagates_a_branch_already_checked_out_error() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateWorktree::new(port);

        let err = use_case
            .execute(
                &sample_repository(),
                Path::new("/repo-wt"),
                WorktreeBranchSpec::ExistingBranch(BranchName::new("main").unwrap()),
            )
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn remove_worktree_delegates_to_port_with_the_force_flag() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = RemoveWorktree::new(port.clone());

        use_case
            .execute(&sample_repository(), Path::new("/repo-wt"), true)
            .unwrap();

        assert_eq!(
            *port.received_remove_worktree.lock().unwrap(),
            Some((PathBuf::from("/repo-wt"), true))
        );
    }

    #[test]
    fn remove_worktree_propagates_uncommitted_changes_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = RemoveWorktree::new(port);

        let err = use_case
            .execute(&sample_repository(), Path::new("/repo-wt"), false)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }
}
