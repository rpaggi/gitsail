//! Stash entity (SAD §8).

use crate::commit::GitTimestamp;
use crate::ids::CommitHash;

/// A single stash entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub index: u32,
    pub commit: CommitHash,
    pub message: String,
    pub date: GitTimestamp,
}
