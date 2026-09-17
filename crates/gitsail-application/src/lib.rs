//! Application use cases and ports for GitSail.
//!
//! Adapters implement ports outside this crate.

#![forbid(unsafe_code)]

pub mod ports;
pub mod use_cases;

pub use ports::{CommitQuery, DiffRequest, Page, RepositoryReadPort};
pub use use_cases::{
    GetCommit, GetCommitHistory, GetDiff, GetFileBlame, GetRepositoryStatus, ListBranches,
    OpenRepository,
};
