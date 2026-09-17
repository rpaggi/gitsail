//! Line/range history entities (SAD §8; US-019).

use std::path::PathBuf;

use crate::blame::LineRange;
use crate::commit::Commit;
use crate::diff::DiffHunk;
use crate::ids::CommitHash;

/// One commit's contribution to the tracked range's evolution (US-019
/// criterion 2): `hunks` shows exactly what changed within the range at
/// this commit, in the same shape as an ordinary diff hunk, so a caller can
/// render it without a separate diff query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHistoryEntry {
    pub commit: Commit,
    pub hunks: Vec<DiffHunk>,
}

/// Commit-level history for a line range of a file, as of a given revision
/// (US-019).
///
/// Unlike [`crate::Blame`], there is no "working tree" option: line history
/// is inherently a question about committed revisions, so `revision` is
/// always a real, resolved commit — never `None` standing in for
/// uncommitted content. This is deliberate (criterion 3): a stretch that
/// only exists as an uncommitted working-tree edit has no commit history to
/// report, and must never be attributed to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHistory {
    pub file: PathBuf,
    pub revision: CommitHash,
    pub range: LineRange,
    pub entries: Vec<LineHistoryEntry>,
}
