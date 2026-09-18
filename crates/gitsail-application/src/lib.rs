//! Application use cases and ports for GitSail.
//!
//! Adapters implement ports outside this crate.

#![forbid(unsafe_code)]

pub mod blame_cache;
pub mod cache;
pub mod concurrency;
pub mod graph_cache;
pub mod mutation;
pub mod patch;
pub mod ports;
pub mod privacy;
pub mod recent_repositories;
pub mod session;
pub mod use_cases;
pub mod write_ports;
pub mod write_use_cases;

pub use blame_cache::{BlameCache, BlameCacheKey, BlameQueryTicket};
pub use cache::{GenerationCache, GenerationTicket};
pub use concurrency::{global_lock_registry, Invalidatable, RepositoryLockRegistry};
pub use graph_cache::{GraphCache, GraphPageKey, DEFAULT_GRAPH_CACHE_CAPACITY};
pub use mutation::{MutationKind, Precondition, RiskLevel};
pub use patch::{export_patch, render_unified_diff, PatchExport};
pub use privacy::{CrashReportConsent, TelemetryPreference};
pub use ports::{BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page, RepositoryReadPort};
pub use recent_repositories::{
    ForgetRecentRepository, ListRecentRepositories, RecentRepositories, RecentRepositoriesPort,
    RecentRepositoryEntry, RecordRecentRepository, MAX_RECENT_REPOSITORIES,
};
pub use session::{RefreshReason, RefreshTicket, RepositorySession, Selection};
pub use use_cases::{
    AmendPreview, CommitDiff, CompareRevisions, DetectInProgressOperation, GetCommit,
    GetCommitDiff, GetCommitHistory, GetConflictSides, GetDiff, GetFileBlame, GetFileContent,
    GetLineHistory, GetRepositoryStatus, ListBranches, OpenRepository, PreviewAmend,
    RevisionComparison,
};
pub use write_ports::{
    ApplyPatchResult, CherryPickResult, MergeParentPolicy, MergeResult, PatchPreview, PullOutcome,
    RebaseAction, RebasePlan, RebasePlanEntry, RebaseResult, RepositoryWritePort, ResetMode,
    RevertResult, StashApplyOutcome, StashScope, TagAnnotation, WorktreeBranchSpec,
};
pub use write_use_cases::{
    AbortOperation, AmendCommit, ApplyPatch, ApplyStash, CherryPick, ContinueOperation,
    CreateBranch, CreateCommit, CreateStash, CreateTag, CreateWorktree, DeleteBranch, DeleteTag,
    DropStash, ExecuteRebasePlan, Fetch, ForcePushWithLease, MarkConflictResolved, Merge,
    PlanRebase, PopStash, PreviewPatchApplication, Pull, Push, Rebase, RemoveWorktree,
    RenameBranch, Reset, Revert, SkipOperation, StageFiles, StageHunks, SwitchBranch,
    TakeConflictSide, UnstageFiles, UnstageHunks,
};
