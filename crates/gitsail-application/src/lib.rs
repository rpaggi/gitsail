//! Application use cases and ports for GitSail.
//!
//! Adapters implement ports outside this crate.

#![forbid(unsafe_code)]

pub mod ports;
pub mod session;
pub mod use_cases;
pub mod write_ports;
pub mod write_use_cases;

pub use ports::{CommitQuery, DiffRequest, Page, RepositoryReadPort};
pub use session::{RefreshReason, RefreshTicket, RepositorySession, Selection};
pub use use_cases::{
    GetCommit, GetCommitHistory, GetDiff, GetFileBlame, GetRepositoryStatus, ListBranches,
    OpenRepository,
};
pub use write_ports::RepositoryWritePort;
pub use write_use_cases::{
    CreateBranch, CreateCommit, DeleteBranch, StageFiles, StageHunks, SwitchBranch, UnstageFiles,
    UnstageHunks,
};
