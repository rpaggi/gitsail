//! Application use cases and ports for GitSail.
//!
//! Adapters implement ports outside this crate.

#![forbid(unsafe_code)]

pub mod blame_cache;
pub mod ports;
pub mod session;
pub mod use_cases;
pub mod write_ports;
pub mod write_use_cases;

pub use blame_cache::{BlameCache, BlameCacheKey, BlameQueryTicket};
pub use ports::{BlameRequest, CommitQuery, DiffRequest, Page, RepositoryReadPort};
pub use session::{RefreshReason, RefreshTicket, RepositorySession, Selection};
pub use use_cases::{
    CommitDiff, CompareRevisions, GetCommit, GetCommitDiff, GetCommitHistory, GetDiff,
    GetFileBlame, GetRepositoryStatus, ListBranches, OpenRepository, RevisionComparison,
};
pub use write_ports::RepositoryWritePort;
pub use write_use_cases::{
    CreateBranch, CreateCommit, DeleteBranch, StageFiles, StageHunks, SwitchBranch, UnstageFiles,
    UnstageHunks,
};
