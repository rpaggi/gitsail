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
pub mod ids;
pub mod line_history;
pub mod remote;
pub mod repository;
pub mod stash;
pub mod status;

pub use blame::{Blame, BlameLine, BlameOrigin, LineRange};
pub use branch::{Branch, BranchKind};
pub use cancellation::CancellationToken;
pub use commit::{Commit, Decoration, GitTimestamp, Signature};
pub use diff::{Diff, DiffHunk, DiffLine, DiffLineOrigin, FileDiff};
pub use error::{ErrorCode, GitSailError, OperationId};
pub use ids::{BranchName, CommitHash, ShortHash};
pub use line_history::{LineHistory, LineHistoryEntry};
pub use remote::{Remote, RemoteUrl};
pub use repository::{HeadState, Repository, RepositoryId};
pub use stash::Stash;
pub use status::{ChangeType, FileChange, FileStatusCode, RepositoryStatus};
