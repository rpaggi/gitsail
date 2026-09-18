//! GitSail domain concepts and invariants.
//!
//! This crate must remain independent from infrastructure and presentation
//! layers: no shell execution, UI toolkit, async runtime or raw Git text
//! parsing lives here (SAD §4). Types are not `Serialize`/`Deserialize` by
//! design — external representation belongs to `gitsail-protocol` (SAD §14).

#![forbid(unsafe_code)]

pub mod blame;
pub mod branch;
pub mod cancellation;
pub mod commit;
pub mod diff;
pub mod error;
pub mod file_content;
pub mod forge;
pub mod graph;
pub mod ids;
pub mod line_history;
pub mod operation;
pub mod redact;
pub mod reflog;
pub mod remote;
pub mod repository;
pub mod sanitize;
pub mod stash;
pub mod status;
pub mod tag;
pub mod worktree;

pub use blame::{Blame, BlameLine, BlameOrigin, LineRange};
pub use branch::{Branch, BranchKind};
pub use cancellation::CancellationToken;
pub use commit::{Commit, Decoration, GitTimestamp, Signature};
pub use diff::{Diff, DiffHunk, DiffLine, DiffLineOrigin, FileDiff};
pub use error::{ErrorCode, GitSailError, OperationId};
pub use file_content::{FileContentAtRevision, FileContentKind};
pub use forge::{build_web_url, detect_forge, repository_location, ForgeKind, ForgePath};
pub use graph::{CommitGraph, GraphCommit, GraphEdge, GraphRow, OpenLane};
pub use ids::{BranchName, CommitHash, ShortHash};
pub use line_history::{LineHistory, LineHistoryEntry};
pub use operation::{
    BisectOperation, ConflictSide, ConflictSideContent, ConflictSides, ConflictStage,
    ConflictedFile, InProgressOperation, MergeOperation, OperationCapability, RebaseOperation,
    SequencerOperation,
};
pub use reflog::{ReflogEntry, ReflogObjectState};
pub use remote::{Remote, RemoteUrl};
pub use repository::{HeadState, Repository, RepositoryId};
pub use stash::Stash;
pub use status::{ChangeType, FileChange, FileStatusCode, RepositoryStatus};
pub use tag::{Tag, TagKind};
pub use worktree::{Worktree, WorktreeHead};
