//! Mutation use cases (SAD §9's v0.2/v0.3 list). Each depends on
//! [`RepositoryWritePort`] rather than a concrete adapter, so it can be
//! exercised with a test double (ADR-002, ADR-009), mirroring
//! `use_cases.rs`'s read-side pattern.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitsail_domain::{
    BranchName, CancellationToken, CommitHash, ConflictSide, FileDiff, GitSailError, Repository,
    Stash, Worktree,
};

use crate::mutation::Precondition;
use crate::write_ports::{
    ApplyPatchResult, MergeResult, PatchPreview, PullOutcome, RebasePlan, RebaseResult,
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

/// Renames a local branch (T-157/US-024). See
/// [`RepositoryWritePort::rename_branch`] for the collision/upstream
/// contract this delegates to unchanged.
pub struct RenameBranch {
    port: Arc<dyn RepositoryWritePort>,
}

impl RenameBranch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        old_name: &BranchName,
        new_name: &BranchName,
    ) -> Result<(), GitSailError> {
        self.port.rename_branch(repo, old_name, new_name)
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

/// Fetches a remote's refs (US-096). See [`RepositoryWritePort::fetch`].
pub struct Fetch {
    port: Arc<dyn RepositoryWritePort>,
}

impl Fetch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        remote: &str,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        self.port.fetch(repo, remote, cancel)
    }
}

/// Integrates a remote branch via a fast-forward-only pull (US-097). See
/// [`RepositoryWritePort::pull`] for the policy this delegates to unchanged.
pub struct Pull {
    port: Arc<dyn RepositoryWritePort>,
}

impl Pull {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<PullOutcome, GitSailError> {
        self.port.pull(repo, remote, branch, cancel)
    }
}

/// Publishes a local branch via a plain push (US-098). See
/// [`RepositoryWritePort::push`].
pub struct Push {
    port: Arc<dyn RepositoryWritePort>,
}

impl Push {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        self.port.push(repo, remote, branch, cancel)
    }
}

/// Force-publishes rewritten history behind a compare-and-swap lease
/// (US-099; `Destructive`, see
/// [`crate::mutation::MutationKind::ForcePushWithLease`]). See
/// [`RepositoryWritePort::force_push_with_lease`].
pub struct ForcePushWithLease {
    port: Arc<dyn RepositoryWritePort>,
}

impl ForcePushWithLease {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        expected_remote_head: &Precondition<CommitHash>,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        self.port
            .force_push_with_lease(repo, remote, branch, expected_remote_head, cancel)
    }
}

/// Previews whether a patch can be applied, without side effects (US-030
/// criterion 1). See [`RepositoryWritePort::preview_patch_application`].
pub struct PreviewPatchApplication {
    port: Arc<dyn RepositoryWritePort>,
}

impl PreviewPatchApplication {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, patch_text: &str) -> Result<PatchPreview, GitSailError> {
        self.port.preview_patch_application(repo, patch_text)
    }
}

/// Applies a patch to the working tree (US-030). See
/// [`RepositoryWritePort::apply_patch`].
pub struct ApplyPatch {
    port: Arc<dyn RepositoryWritePort>,
}

impl ApplyPatch {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, patch_text: &str) -> Result<ApplyPatchResult, GitSailError> {
        self.port.apply_patch(repo, patch_text)
    }
}

/// Integrates a reference into the current branch (T-231/US-079). See
/// [`RepositoryWritePort::merge`] for the fast-forward/merge-commit/conflict
/// contract this delegates to unchanged.
pub struct Merge {
    port: Arc<dyn RepositoryWritePort>,
}

impl Merge {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, target_revision: &str) -> Result<MergeResult, GitSailError> {
        self.port.merge(repo, target_revision)
    }
}

/// Marks one conflicted file resolved by staging it (T-232/US-080). See
/// [`RepositoryWritePort::mark_conflict_resolved`] — always an explicit
/// caller action, never inferred.
pub struct MarkConflictResolved {
    port: Arc<dyn RepositoryWritePort>,
}

impl MarkConflictResolved {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, path: &Path) -> Result<(), GitSailError> {
        self.port.mark_conflict_resolved(repo, path)
    }
}

/// Resolves a conflict by taking one side wholesale (T-232/US-080's
/// documented binary-conflict flow). See
/// [`RepositoryWritePort::take_conflict_side`].
pub struct TakeConflictSide {
    port: Arc<dyn RepositoryWritePort>,
}

impl TakeConflictSide {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, path: &Path, side: ConflictSide) -> Result<(), GitSailError> {
        self.port.take_conflict_side(repo, path, side)
    }
}

/// Resumes whichever operation is currently pending (T-233/US-081). See
/// [`RepositoryWritePort::continue_operation`] for the capability/
/// remaining-conflicts revalidation this delegates to unchanged.
pub struct ContinueOperation {
    port: Arc<dyn RepositoryWritePort>,
}

impl ContinueOperation {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository) -> Result<(), GitSailError> {
        self.port.continue_operation(repo)
    }
}

/// Abandons whichever operation is currently pending (T-233/US-081). See
/// [`RepositoryWritePort::abort_operation`].
pub struct AbortOperation {
    port: Arc<dyn RepositoryWritePort>,
}

impl AbortOperation {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository) -> Result<(), GitSailError> {
        self.port.abort_operation(repo)
    }
}

/// Reapplies the current branch's commits onto another base (T-235/US-083).
/// See [`RepositoryWritePort::rebase`].
pub struct Rebase {
    port: Arc<dyn RepositoryWritePort>,
}

impl Rebase {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, onto_revision: &str) -> Result<RebaseResult, GitSailError> {
        self.port.rebase(repo, onto_revision)
    }
}

/// Skips the current step of whichever operation is pending (T-235/US-083).
/// See [`RepositoryWritePort::skip_operation`].
pub struct SkipOperation {
    port: Arc<dyn RepositoryWritePort>,
}

impl SkipOperation {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository) -> Result<(), GitSailError> {
        self.port.skip_operation(repo)
    }
}

/// Reads a non-mutating interactive rebase plan (T-236/US-084). See
/// [`RepositoryWritePort::plan_rebase`].
pub struct PlanRebase {
    port: Arc<dyn RepositoryWritePort>,
}

impl PlanRebase {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, onto_revision: &str) -> Result<RebasePlan, GitSailError> {
        self.port.plan_rebase(repo, onto_revision)
    }
}

/// Applies a previously built/edited interactive rebase plan (T-236/US-084;
/// T-237/US-085). See [`RepositoryWritePort::execute_rebase_plan`].
pub struct ExecuteRebasePlan {
    port: Arc<dyn RepositoryWritePort>,
}

impl ExecuteRebasePlan {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, plan: &RebasePlan) -> Result<RebaseResult, GitSailError> {
        self.port.execute_rebase_plan(repo, plan)
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
        received_rename_branch: Mutex<Option<(BranchName, BranchName)>>,
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
        received_fetch: Mutex<Option<String>>,
        received_pull: Mutex<Option<(String, BranchName)>>,
        pull_outcome: PullOutcome,
        received_push: Mutex<Option<(String, BranchName)>>,
        received_force_push: Mutex<Option<(String, BranchName, CommitHash)>>,
        received_preview_patch: Mutex<Option<String>>,
        patch_preview: PatchPreview,
        received_apply_patch: Mutex<Option<String>>,
        apply_patch_result: ApplyPatchResult,
        received_merge: Mutex<Option<String>>,
        merge_result: MergeResult,
        received_mark_conflict_resolved: Mutex<Option<PathBuf>>,
        received_continue_operation: Mutex<bool>,
        received_abort_operation: Mutex<bool>,
        received_take_conflict_side: Mutex<Option<(PathBuf, ConflictSide)>>,
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
                received_rename_branch: Mutex::new(None),
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
                received_fetch: Mutex::new(None),
                received_pull: Mutex::new(None),
                pull_outcome: PullOutcome::FastForwarded {
                    new_head: CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap(),
                },
                received_push: Mutex::new(None),
                received_force_push: Mutex::new(None),
                received_preview_patch: Mutex::new(None),
                patch_preview: PatchPreview {
                    affected_files: vec![PathBuf::from("a.txt")],
                    supported: true,
                    rejection_reason: None,
                },
                received_apply_patch: Mutex::new(None),
                apply_patch_result: ApplyPatchResult {
                    applied_files: vec![PathBuf::from("a.txt")],
                },
                received_merge: Mutex::new(None),
                merge_result: MergeResult::FastForwarded {
                    new_head: CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap(),
                },
                received_mark_conflict_resolved: Mutex::new(None),
                received_continue_operation: Mutex::new(false),
                received_abort_operation: Mutex::new(false),
                received_take_conflict_side: Mutex::new(None),
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

        fn rename_branch(
            &self,
            _repo: &Repository,
            old_name: &BranchName,
            new_name: &BranchName,
        ) -> Result<(), GitSailError> {
            *self.received_rename_branch.lock().unwrap() = Some((old_name.clone(), new_name.clone()));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "already exists",
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

        fn fetch(
            &self,
            _repo: &Repository,
            remote: &str,
            _cancel: &CancellationToken,
        ) -> Result<(), GitSailError> {
            *self.received_fetch.lock().unwrap() = Some(remote.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::NetworkFailure,
                    "the remote could not be reached",
                ));
            }
            Ok(())
        }

        fn pull(
            &self,
            _repo: &Repository,
            remote: &str,
            branch: &BranchName,
            _cancel: &CancellationToken,
        ) -> Result<PullOutcome, GitSailError> {
            *self.received_pull.lock().unwrap() = Some((remote.to_string(), branch.clone()));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "local and remote branches have diverged",
                ));
            }
            Ok(self.pull_outcome.clone())
        }

        fn push(
            &self,
            _repo: &Repository,
            remote: &str,
            branch: &BranchName,
            _cancel: &CancellationToken,
        ) -> Result<(), GitSailError> {
            *self.received_push.lock().unwrap() = Some((remote.to_string(), branch.clone()));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "push rejected: remote has commits this branch does not have",
                ));
            }
            Ok(())
        }

        fn force_push_with_lease(
            &self,
            _repo: &Repository,
            remote: &str,
            branch: &BranchName,
            expected_remote_head: &Precondition<CommitHash>,
            _cancel: &CancellationToken,
        ) -> Result<(), GitSailError> {
            *self.received_force_push.lock().unwrap() = Some((
                remote.to_string(),
                branch.clone(),
                expected_remote_head.expected().clone(),
            ));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "force push refused: the remote branch has moved since this lease was captured",
                ));
            }
            Ok(())
        }

        fn preview_patch_application(
            &self,
            _repo: &Repository,
            patch_text: &str,
        ) -> Result<PatchPreview, GitSailError> {
            *self.received_preview_patch.lock().unwrap() = Some(patch_text.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "patch is malformed",
                ));
            }
            Ok(self.patch_preview.clone())
        }

        fn apply_patch(
            &self,
            _repo: &Repository,
            patch_text: &str,
        ) -> Result<ApplyPatchResult, GitSailError> {
            *self.received_apply_patch.lock().unwrap() = Some(patch_text.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "the patch no longer applies to the current file content",
                ));
            }
            Ok(self.apply_patch_result.clone())
        }

        fn merge(&self, _repo: &Repository, target_revision: &str) -> Result<MergeResult, GitSailError> {
            *self.received_merge.lock().unwrap() = Some(target_revision.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "a merge is already in progress",
                ));
            }
            Ok(self.merge_result.clone())
        }

        fn mark_conflict_resolved(&self, _repo: &Repository, path: &Path) -> Result<(), GitSailError> {
            *self.received_mark_conflict_resolved.lock().unwrap() = Some(path.to_path_buf());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "path is not currently conflicted",
                ));
            }
            Ok(())
        }

        fn continue_operation(&self, _repo: &Repository) -> Result<(), GitSailError> {
            *self.received_continue_operation.lock().unwrap() = true;
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "unresolved conflicted files remain",
                ));
            }
            Ok(())
        }

        fn abort_operation(&self, _repo: &Repository) -> Result<(), GitSailError> {
            *self.received_abort_operation.lock().unwrap() = true;
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "no operation is currently in progress",
                ));
            }
            Ok(())
        }

        fn take_conflict_side(
            &self,
            _repo: &Repository,
            path: &Path,
            side: ConflictSide,
        ) -> Result<(), GitSailError> {
            *self.received_take_conflict_side.lock().unwrap() = Some((path.to_path_buf(), side));
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "path is not currently conflicted",
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
    fn rename_branch_delegates_to_port_with_both_names() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = RenameBranch::new(port.clone());
        let old_name = BranchName::new("old-name").unwrap();
        let new_name = BranchName::new("new-name").unwrap();

        use_case
            .execute(&sample_repository(), &old_name, &new_name)
            .unwrap();

        assert_eq!(
            *port.received_rename_branch.lock().unwrap(),
            Some((old_name, new_name))
        );
    }

    #[test]
    fn rename_branch_propagates_port_error_on_name_collision() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = RenameBranch::new(port);

        let err = use_case
            .execute(
                &sample_repository(),
                &BranchName::new("feature").unwrap(),
                &BranchName::new("main").unwrap(),
            )
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
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

    // -----------------------------------------------------------------
    // EPIC-19: fetch, pull, push, force-push-with-lease use cases.
    // -----------------------------------------------------------------

    #[test]
    fn fetch_delegates_to_port_with_the_explicit_remote() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = Fetch::new(port.clone());

        use_case
            .execute(&sample_repository(), "origin", &CancellationToken::new())
            .unwrap();

        assert_eq!(*port.received_fetch.lock().unwrap(), Some("origin".to_string()));
    }

    #[test]
    fn fetch_propagates_a_network_failure_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = Fetch::new(port);

        let err = use_case
            .execute(&sample_repository(), "origin", &CancellationToken::new())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::NetworkFailure);
    }

    #[test]
    fn pull_delegates_to_port_with_remote_and_branch_and_returns_its_outcome() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = Pull::new(port.clone());
        let branch = BranchName::new("main").unwrap();

        let outcome = use_case
            .execute(
                &sample_repository(),
                "origin",
                &branch,
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(outcome, port.pull_outcome);
        assert_eq!(
            *port.received_pull.lock().unwrap(),
            Some(("origin".to_string(), branch))
        );
    }

    #[test]
    fn pull_propagates_a_divergence_conflict_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = Pull::new(port);

        let err = use_case
            .execute(
                &sample_repository(),
                "origin",
                &BranchName::new("main").unwrap(),
                &CancellationToken::new(),
            )
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn push_delegates_to_port_with_the_explicit_remote_and_branch() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = Push::new(port.clone());
        let branch = BranchName::new("feature").unwrap();

        use_case
            .execute(
                &sample_repository(),
                "origin",
                &branch,
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(
            *port.received_push.lock().unwrap(),
            Some(("origin".to_string(), branch))
        );
    }

    #[test]
    fn push_propagates_a_non_fast_forward_rejection_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = Push::new(port);

        let err = use_case
            .execute(
                &sample_repository(),
                "origin",
                &BranchName::new("feature").unwrap(),
                &CancellationToken::new(),
            )
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn force_push_with_lease_delegates_to_port_with_the_expected_remote_head() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = ForcePushWithLease::new(port.clone());
        let branch = BranchName::new("feature").unwrap();
        let expected =
            Precondition::new(CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap());

        use_case
            .execute(
                &sample_repository(),
                "origin",
                &branch,
                &expected,
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(
            *port.received_force_push.lock().unwrap(),
            Some(("origin".to_string(), branch, expected.expected().clone()))
        );
    }

    #[test]
    fn force_push_with_lease_propagates_a_stale_lease_rejection_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = ForcePushWithLease::new(port);
        let expected =
            Precondition::new(CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap());

        let err = use_case
            .execute(
                &sample_repository(),
                "origin",
                &BranchName::new("feature").unwrap(),
                &expected,
                &CancellationToken::new(),
            )
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -----------------------------------------------------------------
    // T-163/US-030: preview/apply patch use cases.
    // -----------------------------------------------------------------

    #[test]
    fn preview_patch_application_delegates_to_port_with_the_exact_patch_text() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = PreviewPatchApplication::new(port.clone());
        let patch_text = "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";

        let preview = use_case.execute(&sample_repository(), patch_text).unwrap();

        assert_eq!(preview, port.patch_preview);
        assert_eq!(
            *port.received_preview_patch.lock().unwrap(),
            Some(patch_text.to_string())
        );
    }

    #[test]
    fn preview_patch_application_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = PreviewPatchApplication::new(port);

        let err = use_case
            .execute(&sample_repository(), "not a patch")
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn apply_patch_delegates_to_port_and_returns_the_applied_files() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = ApplyPatch::new(port.clone());
        let patch_text = "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";

        let result = use_case.execute(&sample_repository(), patch_text).unwrap();

        assert_eq!(result, port.apply_patch_result);
        assert_eq!(
            *port.received_apply_patch.lock().unwrap(),
            Some(patch_text.to_string())
        );
    }

    #[test]
    fn apply_patch_propagates_a_stale_context_conflict_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = ApplyPatch::new(port);

        let err = use_case
            .execute(&sample_repository(), "--- a/a.txt\n+++ b/a.txt\n")
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -----------------------------------------------------------------
    // EPIC-16/T-231..T-233: merge, mark-conflict-resolved, continue/abort.
    // -----------------------------------------------------------------

    #[test]
    fn merge_delegates_to_port_with_the_target_revision_and_returns_its_result() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = Merge::new(port.clone());

        let result = use_case.execute(&sample_repository(), "feature/x").unwrap();

        assert_eq!(result, port.merge_result);
        assert_eq!(
            *port.received_merge.lock().unwrap(),
            Some("feature/x".to_string())
        );
    }

    #[test]
    fn merge_can_report_a_merge_commit_or_a_conflict_as_distinct_results() {
        let mut fake = FakeWritePort::new();
        fake.merge_result = MergeResult::MergeCommitCreated {
            hash: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
        };
        let use_case = Merge::new(Arc::new(fake));
        let commit_result = use_case.execute(&sample_repository(), "feature/x").unwrap();
        assert!(matches!(commit_result, MergeResult::MergeCommitCreated { .. }));

        let mut fake = FakeWritePort::new();
        fake.merge_result = MergeResult::Conflict {
            files: vec![gitsail_domain::ConflictedFile {
                path: PathBuf::from("a.txt"),
                stage: gitsail_domain::ConflictStage::BothModified,
            }],
        };
        let use_case = Merge::new(Arc::new(fake));
        let conflict_result = use_case.execute(&sample_repository(), "feature/x").unwrap();
        match conflict_result {
            MergeResult::Conflict { files } => assert_eq!(files.len(), 1),
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_propagates_an_already_in_progress_conflict_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = Merge::new(port);

        let err = use_case
            .execute(&sample_repository(), "feature/x")
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn mark_conflict_resolved_delegates_to_port_with_the_exact_path() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = MarkConflictResolved::new(port.clone());

        use_case
            .execute(&sample_repository(), Path::new("a.txt"))
            .unwrap();

        assert_eq!(
            *port.received_mark_conflict_resolved.lock().unwrap(),
            Some(PathBuf::from("a.txt"))
        );
    }

    #[test]
    fn mark_conflict_resolved_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = MarkConflictResolved::new(port);

        let err = use_case
            .execute(&sample_repository(), Path::new("a.txt"))
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn continue_operation_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = ContinueOperation::new(port.clone());

        use_case.execute(&sample_repository()).unwrap();

        assert!(*port.received_continue_operation.lock().unwrap());
    }

    #[test]
    fn continue_operation_propagates_a_remaining_conflicts_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = ContinueOperation::new(port);

        let err = use_case.execute(&sample_repository()).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn abort_operation_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = AbortOperation::new(port.clone());

        use_case.execute(&sample_repository()).unwrap();

        assert!(*port.received_abort_operation.lock().unwrap());
    }

    #[test]
    fn abort_operation_propagates_a_nothing_to_abort_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = AbortOperation::new(port);

        let err = use_case.execute(&sample_repository()).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn take_conflict_side_delegates_to_port_with_the_path_and_side() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = TakeConflictSide::new(port.clone());

        use_case
            .execute(&sample_repository(), Path::new("image.png"), ConflictSide::Theirs)
            .unwrap();

        assert_eq!(
            *port.received_take_conflict_side.lock().unwrap(),
            Some((PathBuf::from("image.png"), ConflictSide::Theirs))
        );
    }

    #[test]
    fn take_conflict_side_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = TakeConflictSide::new(port);

        let err = use_case
            .execute(&sample_repository(), Path::new("image.png"), ConflictSide::Ours)
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    // -----------------------------------------------------------------
    // EPIC-17/T-235..T-237: Rebase, SkipOperation, PlanRebase,
    // ExecuteRebasePlan. A dedicated, smaller fake — `FakeWritePort` above
    // predates this epic and does not override these methods (they would
    // just hit the trait's own `unsupported` default), so a fresh double
    // exercises real delegation instead.
    // -----------------------------------------------------------------

    struct FakeRebasePort {
        fail: bool,
        received_rebase_onto: Mutex<Option<String>>,
        rebase_result: RebaseResult,
        received_skip: Mutex<bool>,
        received_plan_onto: Mutex<Option<String>>,
        plan: RebasePlan,
        received_execute_plan: Mutex<Option<RebasePlan>>,
        execute_result: RebaseResult,
    }

    fn sample_rebase_plan() -> RebasePlan {
        RebasePlan {
            onto_revision: "main".to_string(),
            onto: CommitHash::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
            branch_head: CommitHash::new("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap(),
            entries: vec![],
        }
    }

    impl FakeRebasePort {
        fn new() -> Self {
            Self {
                fail: false,
                received_rebase_onto: Mutex::new(None),
                rebase_result: RebaseResult::Completed {
                    new_head: CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap(),
                },
                received_skip: Mutex::new(false),
                received_plan_onto: Mutex::new(None),
                plan: sample_rebase_plan(),
                received_execute_plan: Mutex::new(None),
                execute_result: RebaseResult::Completed {
                    new_head: CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap(),
                },
            }
        }

        fn failing() -> Self {
            Self {
                fail: true,
                ..Self::new()
            }
        }
    }

    impl RepositoryWritePort for FakeRebasePort {
        fn stage_files(&self, _repo: &Repository, _paths: &[PathBuf]) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn unstage_files(&self, _repo: &Repository, _paths: &[PathBuf]) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn create_commit(&self, _repo: &Repository, _message: &str) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn stage_hunks(&self, _repo: &Repository, _selection: &[FileDiff]) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn unstage_hunks(&self, _repo: &Repository, _selection: &[FileDiff]) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn switch_branch(&self, _repo: &Repository, _target: &BranchName) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn create_branch(
            &self,
            _repo: &Repository,
            _name: &BranchName,
            _start_point: Option<&CommitHash>,
        ) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn delete_branch(
            &self,
            _repo: &Repository,
            _name: &BranchName,
            _force: bool,
        ) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn rename_branch(
            &self,
            _repo: &Repository,
            _old_name: &BranchName,
            _new_name: &BranchName,
        ) -> Result<(), GitSailError> {
            unimplemented!("not exercised by these tests")
        }
        fn amend_commit(
            &self,
            _repo: &Repository,
            _message: &str,
            _expected_head: &CommitHash,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn rebase(&self, _repo: &Repository, onto_revision: &str) -> Result<RebaseResult, GitSailError> {
            *self.received_rebase_onto.lock().unwrap() = Some(onto_revision.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "dirty working tree"));
            }
            Ok(self.rebase_result.clone())
        }

        fn skip_operation(&self, _repo: &Repository) -> Result<(), GitSailError> {
            *self.received_skip.lock().unwrap() = true;
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "skip is unsupported for this operation",
                ));
            }
            Ok(())
        }

        fn plan_rebase(&self, _repo: &Repository, onto_revision: &str) -> Result<RebasePlan, GitSailError> {
            *self.received_plan_onto.lock().unwrap() = Some(onto_revision.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::RepositoryNotFound, "bad revision"));
            }
            Ok(self.plan.clone())
        }

        fn execute_rebase_plan(
            &self,
            _repo: &Repository,
            plan: &RebasePlan,
        ) -> Result<RebaseResult, GitSailError> {
            *self.received_execute_plan.lock().unwrap() = Some(plan.clone());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "plan is stale"));
            }
            Ok(self.execute_result.clone())
        }
    }

    #[test]
    fn rebase_delegates_to_port_with_the_exact_onto_revision() {
        let port = Arc::new(FakeRebasePort::new());
        let use_case = Rebase::new(port.clone());

        let result = use_case.execute(&sample_repository(), "main").unwrap();

        assert_eq!(*port.received_rebase_onto.lock().unwrap(), Some("main".to_string()));
        assert_eq!(result, port.rebase_result);
    }

    #[test]
    fn rebase_propagates_a_port_error_without_a_false_success() {
        let port = Arc::new(FakeRebasePort::failing());
        let use_case = Rebase::new(port);

        let err = use_case.execute(&sample_repository(), "main").unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn skip_operation_delegates_to_port() {
        let port = Arc::new(FakeRebasePort::new());
        let use_case = SkipOperation::new(port.clone());

        use_case.execute(&sample_repository()).unwrap();

        assert!(*port.received_skip.lock().unwrap());
    }

    #[test]
    fn skip_operation_propagates_an_unsupported_error_without_a_false_success() {
        let port = Arc::new(FakeRebasePort::failing());
        let use_case = SkipOperation::new(port);

        let err = use_case.execute(&sample_repository()).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn plan_rebase_delegates_to_port_and_returns_its_plan() {
        let port = Arc::new(FakeRebasePort::new());
        let use_case = PlanRebase::new(port.clone());

        let plan = use_case.execute(&sample_repository(), "develop").unwrap();

        assert_eq!(
            *port.received_plan_onto.lock().unwrap(),
            Some("develop".to_string())
        );
        assert_eq!(plan, port.plan);
    }

    #[test]
    fn execute_rebase_plan_delegates_to_port_with_the_exact_plan() {
        let port = Arc::new(FakeRebasePort::new());
        let use_case = ExecuteRebasePlan::new(port.clone());
        let plan = sample_rebase_plan();

        let result = use_case.execute(&sample_repository(), &plan).unwrap();

        assert_eq!(*port.received_execute_plan.lock().unwrap(), Some(plan));
        assert_eq!(result, port.execute_result);
    }

    #[test]
    fn execute_rebase_plan_propagates_a_stale_plan_error_without_a_false_success() {
        let port = Arc::new(FakeRebasePort::failing());
        let use_case = ExecuteRebasePlan::new(port);

        let err = use_case
            .execute(&sample_repository(), &sample_rebase_plan())
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }
}
