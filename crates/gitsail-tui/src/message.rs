//! Typed results flowing back onto the main loop's single channel — either
//! a terminal input event or the outcome of a background [`crate::worker::Command`]
//! (SAD §18: "... return typed messages/events").

use gitsail_application::{
    ApplyPatchResult, CherryPickResult, MergeResult, Page, PatchPreview, PullOutcome, RebasePlan,
    RebaseResult, RefreshTicket, RevertResult,
};
use gitsail_domain::{
    Blame, Branch, Commit, CommitHash, ConflictSides, Diff, GitSailError, InProgressOperation,
    Remote, Repository, RepositoryStatus, Stash, Tag,
};

#[derive(Debug)]
pub enum Message {
    /// A raw terminal event, forwarded by [`crate::event::spawn`].
    Term(crossterm::event::Event),
    /// A periodic wake-up with no event, so the render loop is never
    /// blocked indefinitely on `crossterm::event::read`.
    Tick,
    /// [`crate::worker::Command::OpenRepository`] completed.
    RepositoryOpened(Result<Repository, GitSailError>),
    /// [`crate::worker::Command::RefreshStatus`] completed. Tagged with the
    /// [`RefreshTicket`] it was issued for, so [`crate::app::App`] can
    /// discard a stale result exactly like
    /// [`gitsail_application::RepositorySession::apply_refresh`] already
    /// does for a successful one (US-041 criterion 3).
    StatusRefreshed(RefreshTicket, Result<RepositoryStatus, GitSailError>),
    /// [`crate::worker::Command::LoadBranches`] completed. Tagged with the
    /// session generation active when it was requested, for the same
    /// staleness check.
    BranchesLoaded(u64, Result<Vec<Branch>, GitSailError>),
    /// [`crate::worker::Command::LoadDiff`] completed (US-046). Tagged with
    /// the request id issued when it was dispatched, so a diff computed for
    /// a since-abandoned selection is discarded.
    DiffLoaded(u64, Result<Diff, GitSailError>),
    /// [`crate::worker::Command::LoadBlame`] completed (US-046), tagged like
    /// [`Self::DiffLoaded`].
    BlameLoaded(u64, Result<Blame, GitSailError>),
    /// [`crate::worker::Command::LoadCommitGraph`] completed (US-065,
    /// US-066), tagged like [`Self::DiffLoaded`].
    CommitGraphPageLoaded(u64, Result<Page<Commit>, GitSailError>),
    /// A `SwitchBranch`/`CreateBranch`/`DeleteBranch`/`StageFiles`/
    /// `UnstageFiles` [`crate::worker::Command`] completed (US-047, US-048).
    OperationFinished(Result<(), GitSailError>),
    /// [`crate::worker::Command::CreateCommit`] completed (US-047).
    CommitCreated(Result<CommitHash, GitSailError>),
    /// [`crate::worker::Command::LoadTags`] completed (US-050), tagged with
    /// the session generation active when it was requested, matching
    /// [`Self::BranchesLoaded`].
    TagsLoaded(u64, Result<Vec<Tag>, GitSailError>),
    /// [`crate::worker::Command::LoadRemotes`] completed (US-050), tagged
    /// like [`Self::TagsLoaded`].
    RemotesLoaded(u64, Result<Vec<Remote>, GitSailError>),
    /// [`crate::worker::Command::LoadStashEntries`] completed (US-050),
    /// tagged like [`Self::TagsLoaded`].
    StashEntriesLoaded(u64, Result<Vec<Stash>, GitSailError>),
    /// [`crate::worker::Command::Pull`] completed (US-049). Carries the
    /// [`PullOutcome`] on success — distinct from
    /// [`Self::OperationFinished`] so "already up to date" and
    /// "fast-forwarded to `<hash>`" can both be shown explicitly rather than
    /// collapsed into a bare success (criterion 1: "divergência ou conflito
    /// é mostrado claramente").
    PullFinished(Result<PullOutcome, GitSailError>),
    /// [`crate::worker::Command::PreviewPatchApplication`] completed
    /// (T-163/US-030 criterion 1). Carries the patch text alongside the
    /// preview so [`crate::app::App`] can hold onto it for the confirmed
    /// [`crate::worker::Command::ApplyPatch`] that follows, without a
    /// second clipboard read (the clipboard's contents could have changed
    /// in between).
    PatchPreviewed(Result<PatchPreview, GitSailError>, String),
    /// [`crate::worker::Command::ApplyPatch`] completed (T-163/US-030).
    /// Carries the [`ApplyPatchResult`] on success — distinct from
    /// [`Self::OperationFinished`] so the exact applied files can be shown,
    /// mirroring [`Self::PullFinished`]'s own reasoning.
    PatchApplied(Result<ApplyPatchResult, GitSailError>),
    /// [`crate::worker::Command::LoadInProgressOperation`] completed
    /// (T-230/US-078, presentation side of T-231/T-233). Tagged with the
    /// session generation active when it was requested, matching
    /// [`Self::TagsLoaded`]. Loaded after every refresh (and after a merge/
    /// continue/abort mutation) so a merge/conflict/rebase/... started in
    /// another terminal is always picked up (US-078 criterion 2).
    InProgressOperationLoaded(u64, Result<InProgressOperation, GitSailError>),
    /// [`crate::worker::Command::Merge`] completed (T-231/US-079). Carries
    /// the [`MergeResult`] on success — distinct from
    /// [`Self::OperationFinished`] so fast-forward/merge-commit/conflict are
    /// always three distinct, explicit outcomes (US-079 criterion 2), never
    /// collapsed into a bare success/failure.
    MergeFinished(Result<MergeResult, GitSailError>),
    /// [`crate::worker::Command::LoadConflictSides`] completed (T-232/
    /// US-080 criterion 2), tagged with the conflicted path it was requested
    /// for, so a result for a since-abandoned selection can be discarded.
    ConflictSidesLoaded(std::path::PathBuf, Result<ConflictSides, GitSailError>),
    /// [`crate::worker::Command::MarkConflictResolved`]/
    /// [`crate::worker::Command::TakeConflictSide`] completed (T-232/US-080
    /// criterion 3).
    ConflictResolutionFinished(Result<(), GitSailError>),
    /// [`crate::worker::Command::ContinueOperation`]/
    /// [`crate::worker::Command::AbortOperation`] completed (T-233/US-081).
    /// The real resulting state is always reinspected afterward via a fresh
    /// [`Self::InProgressOperationLoaded`] (US-081 criterion 3) — this
    /// message only carries whether the Git command itself exited
    /// successfully, never a presumption that the operation fully concluded.
    OperationResolutionFinished(Result<(), GitSailError>),
    /// [`crate::worker::Command::Rebase`] completed (T-235/US-083). Carries
    /// the [`RebaseResult`] on success — distinct from
    /// [`Self::OperationFinished`] so completion/conflict are always two
    /// distinct, explicit outcomes, never collapsed into a bare
    /// success/failure (mirrors [`Self::MergeFinished`]'s own reasoning).
    RebaseFinished(Result<RebaseResult, GitSailError>),
    /// [`crate::worker::Command::PlanRebase`] completed (T-236/US-084
    /// criterion 1). A load failure is shown inline in the (already open)
    /// overlay rather than through [`crate::operation::OperationState`] —
    /// see [`crate::app::App::on_rebase_plan_loaded`].
    RebasePlanLoaded(Result<RebasePlan, GitSailError>),
    /// [`crate::worker::Command::CherryPick`] completed (T-238/US-086).
    /// Carries the [`CherryPickResult`] on success — distinct from
    /// [`Self::OperationFinished`] so applying/conflict/empty are always
    /// three distinct, explicit outcomes (US-086 criterion 3), never
    /// collapsed into a bare success/failure (mirrors
    /// [`Self::MergeFinished`]'s own reasoning).
    CherryPickFinished(Result<CherryPickResult, GitSailError>),
    /// [`crate::worker::Command::Revert`] completed (T-239/US-087). Carries
    /// the [`RevertResult`] on success, mirroring [`Self::CherryPickFinished`].
    RevertFinished(Result<RevertResult, GitSailError>),
}
