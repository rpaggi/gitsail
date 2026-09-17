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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffLine {
    pub origin: DiffLineOrigin,
    pub content: String,
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
    pub hunks: Vec<DiffHunk>,
}

/// A diff across one or more files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub files: Vec<FileDiff>,
}
