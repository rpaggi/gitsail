//! Branch entity (SAD §8).

use crate::ids::{BranchName, CommitHash};

/// Whether a branch is local or tracks a remote.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BranchKind {
    Local,
    Remote { remote: String },
}

/// A branch reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Branch {
    pub name: BranchName,
    pub kind: BranchKind,
    pub target: CommitHash,
    pub upstream: Option<BranchName>,
    pub ahead: u32,
    pub behind: u32,
    pub is_current: bool,
}
