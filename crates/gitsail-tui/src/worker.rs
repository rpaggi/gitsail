//! Executes [`Command`]s issued by [`crate::app::App::update`] on
//! background threads, so Git process execution never runs on the render
//! loop (SAD §18, §26; US-041 criterion 2).
//!
//! Each `Command` gets its own thread. A `RepositoryReadPort`/
//! `RepositoryWritePort` call already runs at most one `git` process at a
//! time and one frame issues only a couple of these, so a persistent worker
//! pool would be premature machinery for what this epic needs; a future
//! epic issuing many concurrent reads can revisit this without changing the
//! `Command`/`Message` contract.
//!
//! Cancellation (SAD §26: "operações de leitura caras devem suportar
//! cancelamento") is out of scope for the commands added by US-046: no
//! acceptance criterion here asks for a diff/blame in flight to be
//! cancelable, so each read is issued with a fresh, never-cancelled
//! [`CancellationToken`] — a deliberate scope cut, not an oversight.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;

use gitsail_application::{
    AbortOperation, AmendCommit, ApplyPatch, BlameRequest, CherryPick, CommitQuery,
    ContinueOperation, CreateBranch, CreateCommit, DeleteBranch, DetectInProgressOperation,
    DiffRequest, ExecuteRebasePlan, Fetch, GetCommit, GetCommitHistory, GetConflictSides, GetDiff,
    GetFileBlame, GetReflog, GetRepositoryStatus, ListBranches, MarkConflictResolved, Merge,
    MergeParentPolicy, OpenRepository, PlanRebase, PreviewAmend, PreviewPatchApplication, Pull,
    Push, Rebase, RebasePlan, RefreshTicket, RenameBranch, RepositoryReadPort, RepositoryWritePort,
    Reset, ResetMode, Revert, SkipOperation, StageFiles, SwitchBranch, TakeConflictSide,
    UnstageFiles,
};
use gitsail_domain::{BranchName, CancellationToken, CommitHash, ConflictSide, Repository};

use crate::message::Message;

/// A side effect [`crate::app::App::update`] wants run outside itself.
/// `App::update` only ever returns these; it never touches a port or a
/// thread directly, which is what keeps it testable without any I/O.
#[derive(Debug, Clone)]
pub enum Command {
    OpenRepository(PathBuf),
    RefreshStatus(RefreshTicket, Repository),
    LoadBranches(u64, Repository),
    /// Loads a diff for the given request, tagged with a request id so a
    /// result computed for a since-abandoned selection can be discarded
    /// (US-046).
    LoadDiff(u64, Repository, DiffRequest),
    /// Loads blame for the given request, tagged like [`Self::LoadDiff`].
    /// The trailing `u64` is the `content_version` [`GetFileBlame`] uses to
    /// key its cache — the session's refresh generation, so any refresh
    /// invalidates it.
    LoadBlame(u64, Repository, BlameRequest, u64),
    /// Loads one page of commit-graph history (US-065, US-066), tagged
    /// with a request id like [`Self::LoadDiff`]. The `CommitQuery`'s
    /// `cursor` is what makes this "the next page" rather than a restart —
    /// [`crate::app::App`] carries it forward from the previous page's
    /// result.
    LoadCommitGraph(u64, Repository, CommitQuery),
    StageFiles(Repository, Vec<PathBuf>),
    UnstageFiles(Repository, Vec<PathBuf>),
    CreateCommit(Repository, String),
    SwitchBranch(Repository, BranchName),
    CreateBranch(Repository, BranchName, Option<CommitHash>),
    DeleteBranch(Repository, BranchName, bool),
    /// Renames a local branch (T-157/US-024).
    RenameBranch(Repository, BranchName, BranchName),
    /// Loads local tags (US-050), tagged with the session generation active
    /// when it was requested, matching [`Self::LoadBranches`].
    LoadTags(u64, Repository),
    /// Loads configured remotes (US-050), tagged like [`Self::LoadTags`].
    LoadRemotes(u64, Repository),
    /// Loads stash entries (US-050), tagged like [`Self::LoadTags`].
    LoadStashEntries(u64, Repository),
    /// Fetches `remote` (US-049). `Safe`, so `App` dispatches this
    /// immediately without a confirmation step.
    Fetch(Repository, String),
    /// Pulls `remote`'s tracked `branch` into the current branch (US-049),
    /// fast-forward only (see `RepositoryWritePort::pull`'s own doc).
    Pull(Repository, String, BranchName),
    /// Pushes the current `branch` to `remote` (US-049), never forcing.
    Push(Repository, String, BranchName),
    /// Validates a candidate patch via `git apply --check`, without side
    /// effects (T-163/US-030 criterion 1). Dispatched as soon as
    /// `Action::RequestApplyPatch` reads a non-empty clipboard — *before*
    /// any confirmation, since building the preview itself needs no
    /// confirmation (it never mutates anything).
    PreviewPatchApplication(Repository, String),
    /// Applies a previously previewed, now-confirmed patch to the working
    /// tree (T-163/US-030).
    ApplyPatch(Repository, String),
    /// Detects a merge/rebase/cherry-pick/revert/bisect currently in
    /// progress (T-230/US-078, presentation side of T-231/T-233), tagged
    /// with the session generation active when it was requested, matching
    /// [`Self::LoadTags`]. `Safe`, so `App` dispatches this immediately
    /// without a confirmation step, exactly like [`Self::LoadStashEntries`].
    LoadInProgressOperation(u64, Repository),
    /// Integrates `target_revision` into the current branch (T-231/US-079).
    Merge(Repository, String),
    /// Reads one conflicted file's base/ours/theirs sides (T-232/US-080
    /// criterion 2), tagged with the exact path requested.
    LoadConflictSides(Repository, PathBuf),
    /// Stages a conflicted file as resolved (T-232/US-080 criterion 3).
    MarkConflictResolved(Repository, PathBuf),
    /// Resolves a conflicted file by taking one side wholesale — the
    /// documented binary-conflict flow (T-232/US-080 criterion 3).
    TakeConflictSide(Repository, PathBuf, ConflictSide),
    /// Resumes whichever operation is currently pending (T-233/US-081).
    ContinueOperation(Repository),
    /// Abandons whichever operation is currently pending (T-233/US-081).
    AbortOperation(Repository),
    /// Rebases the current branch onto the given revision (T-235/US-083).
    Rebase(Repository, String),
    /// Skips the current step of whichever operation is pending (T-235/
    /// US-083).
    SkipOperation(Repository),
    /// Reads a non-mutating interactive rebase plan for the candidate range
    /// the current branch would reapply onto `onto_revision` (T-236/US-084
    /// criterion 1).
    PlanRebase(Repository, String),
    /// Applies a previously built/edited interactive rebase plan (T-236/
    /// US-084; T-237/US-085's squash/fixup are just two of this same plan's
    /// actions).
    ExecuteRebasePlan(Repository, RebasePlan),
    /// Cherry-picks `commit` onto the current branch (T-238/US-086).
    /// `merge_parent` is only ever `Some` for a merge commit (see
    /// `gitsail_application::MergeParentPolicy`'s own doc).
    CherryPick(Repository, CommitHash, Option<MergeParentPolicy>),
    /// Reverts `commit`'s change as a new commit on the current branch
    /// (T-239/US-087). `merge_parent` mirrors [`Self::CherryPick`]'s own
    /// contract.
    Revert(Repository, CommitHash, Option<MergeParentPolicy>),
    /// Resets `HEAD`/the index/the working tree (per `mode`) to a target
    /// revision (T-240/US-088), revalidating `expected_head` immediately
    /// before running.
    Reset(Repository, String, ResetMode, CommitHash),
    /// Loads `HEAD`'s reflog entries (T-241/US-089), tagged with the session
    /// generation active when it was requested, matching [`Self::LoadTags`].
    LoadReflog(u64, Repository),
    /// Loads one reflog entry's full commit details (T-241/US-089 criterion
    /// 2), via the same [`GetCommit`] use case the Graph/References panels
    /// already reuse — never a parallel read. Tagged with the exact hash
    /// requested, matching [`Self::LoadConflictSides`]'s own path tagging.
    LoadReflogCommit(Repository, CommitHash),
    /// Builds a non-mutating amend preview (T-242/US-090 criterion 1) via
    /// `gitsail_application::PreviewAmend` — the same use case
    /// `apps/desktop`'s amend flow already uses (T-192), never a parallel
    /// read.
    PreviewAmend(Repository),
    /// Replaces `HEAD`'s commit with `message` and whatever is currently
    /// staged (T-242/US-090), revalidating `expected_head` immediately
    /// before amending — via `gitsail_application::AmendCommit`, the same
    /// use case `apps/desktop`'s amend flow already uses (T-192), never a
    /// parallel Git implementation.
    AmendCommit(Repository, String, CommitHash),
}

/// Spawns one background thread per command in `commands`, each reporting
/// its result back on `tx` as a [`Message`].
pub fn dispatch(
    commands: Vec<Command>,
    read_port: &Arc<dyn RepositoryReadPort>,
    write_port: &Arc<dyn RepositoryWritePort>,
    tx: &Sender<Message>,
) {
    for command in commands {
        spawn_one(
            command,
            Arc::clone(read_port),
            Arc::clone(write_port),
            tx.clone(),
        );
    }
}

fn spawn_one(
    command: Command,
    read_port: Arc<dyn RepositoryReadPort>,
    write_port: Arc<dyn RepositoryWritePort>,
    tx: Sender<Message>,
) {
    thread::spawn(move || {
        let message = match command {
            Command::OpenRepository(path) => {
                let result = OpenRepository::new(read_port).execute(&path);
                Message::RepositoryOpened(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(read_port).execute(&repo);
                Message::StatusRefreshed(ticket, result)
            }
            Command::LoadBranches(generation, repo) => {
                let result = ListBranches::new(read_port).execute(&repo);
                Message::BranchesLoaded(generation, result)
            }
            Command::LoadDiff(request_id, repo, request) => {
                let result =
                    GetDiff::new(read_port).execute(&repo, &request, &CancellationToken::new());
                Message::DiffLoaded(request_id, result)
            }
            Command::LoadBlame(request_id, repo, request, content_version) => {
                let result = GetFileBlame::new(read_port).execute(
                    &repo,
                    &request,
                    content_version,
                    &CancellationToken::new(),
                );
                Message::BlameLoaded(request_id, result)
            }
            Command::LoadCommitGraph(request_id, repo, query) => {
                let result = GetCommitHistory::new(read_port).execute(&repo, &query);
                Message::CommitGraphPageLoaded(request_id, result)
            }
            Command::StageFiles(repo, paths) => {
                let result = StageFiles::new(write_port).execute(&repo, &paths);
                Message::OperationFinished(result)
            }
            Command::UnstageFiles(repo, paths) => {
                let result = UnstageFiles::new(write_port).execute(&repo, &paths);
                Message::OperationFinished(result)
            }
            Command::CreateCommit(repo, message_text) => {
                let result = CreateCommit::new(write_port).execute(&repo, &message_text);
                Message::CommitCreated(result)
            }
            Command::SwitchBranch(repo, target) => {
                let result = SwitchBranch::new(write_port).execute(&repo, &target);
                Message::OperationFinished(result)
            }
            Command::CreateBranch(repo, name, start_point) => {
                let result =
                    CreateBranch::new(write_port).execute(&repo, &name, start_point.as_ref());
                Message::OperationFinished(result)
            }
            Command::DeleteBranch(repo, name, force) => {
                let result = DeleteBranch::new(write_port).execute(&repo, &name, force);
                Message::OperationFinished(result)
            }
            Command::RenameBranch(repo, old_name, new_name) => {
                let result = RenameBranch::new(write_port).execute(&repo, &old_name, &new_name);
                Message::OperationFinished(result)
            }
            Command::LoadTags(generation, repo) => {
                let result = read_port.list_tags(&repo);
                Message::TagsLoaded(generation, result)
            }
            Command::LoadRemotes(generation, repo) => {
                let result = read_port.list_remotes(&repo);
                Message::RemotesLoaded(generation, result)
            }
            Command::LoadStashEntries(generation, repo) => {
                let result = read_port.list_stash_entries(&repo);
                Message::StashEntriesLoaded(generation, result)
            }
            Command::Fetch(repo, remote) => {
                let result = Fetch::new(write_port).execute(&repo, &remote, &CancellationToken::new());
                Message::OperationFinished(result)
            }
            Command::Pull(repo, remote, branch) => {
                let result =
                    Pull::new(write_port).execute(&repo, &remote, &branch, &CancellationToken::new());
                Message::PullFinished(result)
            }
            Command::Push(repo, remote, branch) => {
                let result =
                    Push::new(write_port).execute(&repo, &remote, &branch, &CancellationToken::new());
                Message::OperationFinished(result)
            }
            Command::PreviewPatchApplication(repo, patch_text) => {
                let result = PreviewPatchApplication::new(write_port).execute(&repo, &patch_text);
                Message::PatchPreviewed(result, patch_text)
            }
            Command::ApplyPatch(repo, patch_text) => {
                let result = ApplyPatch::new(write_port).execute(&repo, &patch_text);
                Message::PatchApplied(result)
            }
            Command::LoadInProgressOperation(generation, repo) => {
                let result = DetectInProgressOperation::new(read_port).execute(&repo);
                Message::InProgressOperationLoaded(generation, result)
            }
            Command::Merge(repo, target_revision) => {
                let result = Merge::new(write_port).execute(&repo, &target_revision);
                Message::MergeFinished(result)
            }
            Command::LoadConflictSides(repo, path) => {
                let result = GetConflictSides::new(read_port).execute(&repo, &path);
                Message::ConflictSidesLoaded(path, result)
            }
            Command::MarkConflictResolved(repo, path) => {
                let result = MarkConflictResolved::new(write_port).execute(&repo, &path);
                Message::ConflictResolutionFinished(result)
            }
            Command::TakeConflictSide(repo, path, side) => {
                let result = TakeConflictSide::new(write_port).execute(&repo, &path, side);
                Message::ConflictResolutionFinished(result)
            }
            Command::ContinueOperation(repo) => {
                let result = ContinueOperation::new(write_port).execute(&repo);
                Message::OperationResolutionFinished(result)
            }
            Command::AbortOperation(repo) => {
                let result = AbortOperation::new(write_port).execute(&repo);
                Message::OperationResolutionFinished(result)
            }
            Command::Rebase(repo, onto_revision) => {
                let result = Rebase::new(write_port).execute(&repo, &onto_revision);
                Message::RebaseFinished(result)
            }
            Command::SkipOperation(repo) => {
                let result = SkipOperation::new(write_port).execute(&repo);
                Message::OperationResolutionFinished(result)
            }
            Command::PlanRebase(repo, onto_revision) => {
                let result = PlanRebase::new(write_port).execute(&repo, &onto_revision);
                Message::RebasePlanLoaded(result)
            }
            Command::ExecuteRebasePlan(repo, plan) => {
                let result = ExecuteRebasePlan::new(write_port).execute(&repo, &plan);
                Message::RebaseFinished(result)
            }
            Command::CherryPick(repo, commit, merge_parent) => {
                let result = CherryPick::new(write_port).execute(&repo, &commit, merge_parent);
                Message::CherryPickFinished(result)
            }
            Command::Revert(repo, commit, merge_parent) => {
                let result = Revert::new(write_port).execute(&repo, &commit, merge_parent);
                Message::RevertFinished(result)
            }
            Command::Reset(repo, target_revision, mode, expected_head) => {
                let result = Reset::new(write_port).execute(&repo, &target_revision, mode, &expected_head);
                Message::OperationFinished(result)
            }
            Command::LoadReflog(generation, repo) => {
                let result = GetReflog::new(read_port).execute(&repo);
                Message::ReflogLoaded(generation, result)
            }
            Command::LoadReflogCommit(repo, hash) => {
                let result = GetCommit::new(read_port).execute(&repo, &hash);
                Message::ReflogCommitLoaded(hash, result)
            }
            Command::PreviewAmend(repo) => {
                let result = PreviewAmend::new(read_port).execute(&repo, &CancellationToken::new());
                Message::AmendPreviewed(result)
            }
            Command::AmendCommit(repo, message, expected_head) => {
                let result = AmendCommit::new(write_port).execute(&repo, &message, &expected_head);
                Message::AmendCommitFinished(result)
            }
        };
        // The receiving end only disappears once the app is shutting down
        // (the main loop dropped its `Receiver`); a job finishing after
        // that has nothing useful left to report, so a send failure here
        // is expected and not an error to surface anywhere.
        let _ = tx.send(message);
    });
}
