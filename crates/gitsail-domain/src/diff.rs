//! Diff entities (SAD §8).

use std::path::PathBuf;

use crate::status::ChangeType;

/// The role a line plays within a diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffLineOrigin {
    Context,
    Addition,
    Deletion,
}

/// A single line within a [`DiffHunk`].
///
/// `content` carries the line's bytes exactly as Git reported them,
/// including a trailing `\r` for a CRLF line — there is no separate
/// line-ending enum, so a CRLF file's lines round-trip losslessly through
/// this type (US-027 criterion 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffLine {
    pub origin: DiffLineOrigin,
    pub content: String,
    /// `false` when this is the last line of its file and that file has no
    /// trailing newline (Git's `\ No newline at end of file` marker) —
    /// distinct from "line was deleted", so a caller renders/applies it
    /// correctly instead of silently adding a newline that was never there
    /// (US-027 criterion 2).
    pub has_trailing_newline: bool,
}

/// A contiguous block of changed lines with surrounding context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// The diff for a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub change_type: ChangeType,
    pub is_binary: bool,
    /// `true` when this file's content diff exceeded the adapter's size
    /// limit and `hunks` was withheld rather than parsed (US-027 criterion
    /// 3). Callers must not read a truncated `FileDiff` with empty `hunks`
    /// as "no changes" — the change is real, its content is just
    /// unavailable at this size.
    pub truncated: bool,
    pub hunks: Vec<DiffHunk>,
}

/// A diff across one or more files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub files: Vec<FileDiff>,
}
