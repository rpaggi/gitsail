//! Blame entities (SAD §8).

use std::path::PathBuf;

use crate::commit::{GitTimestamp, Signature};
use crate::ids::CommitHash;

/// A 1-indexed, inclusive line range in the queried revision's final line
/// numbering, as accepted by a blame query (US-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Whether the range is structurally well-formed: both bounds start
    /// from line 1, and `start` does not come after `end`. This does not
    /// know whether the range fits within any particular file — that is a
    /// property of the file at the queried revision, checked by the
    /// adapter against the actual line count (US-032 criterion 3).
    pub fn is_valid(&self) -> bool {
        self.start >= 1 && self.start <= self.end
    }
}

/// Whether a blame line is attributed to a real, historical commit, or
/// reflects working-tree content that has not been committed yet — Git's
/// own "not committed yet" attribution, surfaced as its own domain state
/// rather than a commit a caller could mistake for a real one (US-033
/// criterion 1: "linhas alteradas localmente têm estado próprio").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlameOrigin {
    Committed,
    Local,
}

#[cfg(test)]
mod line_range_tests {
    use super::*;

    // T-253/US-120 criterion 1: closes the one domain invariant gap found in
    // this crate that predated this story — every other `pub fn new`/
    // validation-bearing type here already had unit tests; `LineRange` did
    // not.

    #[test]
    fn a_single_line_range_is_valid() {
        assert!(LineRange::new(1, 1).is_valid());
    }

    #[test]
    fn an_ascending_range_is_valid() {
        assert!(LineRange::new(1, 10).is_valid());
        assert!(LineRange::new(5, 10).is_valid());
    }

    #[test]
    fn a_range_starting_before_line_one_is_invalid() {
        assert!(!LineRange::new(0, 5).is_valid());
    }

    #[test]
    fn a_range_whose_start_comes_after_its_end_is_invalid() {
        assert!(!LineRange::new(10, 5).is_valid());
    }

    #[test]
    fn is_valid_never_checks_against_any_particular_files_line_count() {
        // Deliberately: `LineRange` alone cannot know whether a range fits a
        // given file at a given revision — that is the adapter's job,
        // checked against the real line count (see this type's own doc).
        // An enormous, structurally well-formed range is still `is_valid`.
        assert!(LineRange::new(1, u32::MAX).is_valid());
    }
}

/// A single attributed line of blame output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub final_line: u32,
    pub original_line: u32,
    pub commit: CommitHash,
    pub author: Signature,
    pub timestamp: GitTimestamp,
    pub content: String,
    pub origin: BlameOrigin,
}

/// Line-by-line attribution for a file at a given revision.
///
/// `file` and `revision` echo back what was actually queried — `revision:
/// None` means the working tree — so a caller can always identify which
/// content and revision a result belongs to (US-033 criterion 2: "conteúdo
/// consultado e revisão são identificáveis pela interface") rather than
/// inferring it from context that may have since changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blame {
    pub file: PathBuf,
    pub revision: Option<CommitHash>,
    pub lines: Vec<BlameLine>,
}
