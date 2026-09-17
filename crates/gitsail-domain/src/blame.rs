//! Blame entities (SAD §8).

use crate::commit::{GitTimestamp, Signature};
use crate::ids::CommitHash;

/// A single attributed line of blame output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub final_line: u32,
    pub original_line: u32,
    pub commit: CommitHash,
    pub author: Signature,
    pub timestamp: GitTimestamp,
    pub content: String,
}

/// Line-by-line attribution for a file at a given revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blame {
    pub lines: Vec<BlameLine>,
}
