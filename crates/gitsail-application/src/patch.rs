//! Unified-diff patch rendering and export (US-013, US-029/T-162).
//!
//! [`render_unified_diff`] is the single renderer both callers share:
//! `gitsail-git`'s hunk-level stage/unstage (US-013) renders a *subset* of
//! hunks it is about to `git apply --cached`, and [`export_patch`] (US-029)
//! renders the *whole* visible [`FileDiff`] set for a person to copy or
//! save. Both need the exact same `git apply`-compatible framing, so the
//! text-formatting logic lives here, once, in the layer both an adapter
//! (`gitsail-git`) and a presentation layer (TUI, Desktop) can reach without
//! either depending on the other.
//!
//! Deliberately free functions rather than a port-backed use case: unlike
//! every other type in this module, patch rendering does no I/O and touches
//! no [`crate::ports::RepositoryReadPort`] — it is a pure transform over
//! [`FileDiff`] data a caller already fetched, so a `struct` wrapping a port
//! would only add ceremony.

use std::path::PathBuf;

use gitsail_domain::{ChangeType, DiffLineOrigin, FileDiff};

/// Renders `files` into `git apply`-compatible unified diff text: the
/// minimal `---`/`+++`/`@@` framing `git apply` accepts, without a
/// `diff --git`/`index` header — none of that is needed to apply content
/// hunks, and neither caller of this function ever needs to *parse* what it
/// renders here.
///
/// A file with no hunks (a binary change, a truncated diff withholding its
/// hunks, or a pure rename with no content change) contributes nothing to
/// the output — never a fabricated, empty hunk. A line with
/// `has_trailing_newline: false` is rendered with the `\ No newline at end
/// of file` marker `git apply` expects instead of a synthesized trailing
/// newline (US-027 criterion 2), so a round trip through this renderer and
/// back through `git apply` reproduces the original bytes exactly.
pub fn render_unified_diff(files: &[FileDiff]) -> String {
    let mut out = String::new();
    for file in files {
        if file.hunks.is_empty() {
            continue;
        }
        let (old_path, new_path) = patch_paths(file);
        out.push_str(&format!("--- {old_path}\n"));
        out.push_str(&format!("+++ {new_path}\n"));
        for hunk in &file.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                let sigil = match line.origin {
                    DiffLineOrigin::Context => ' ',
                    DiffLineOrigin::Addition => '+',
                    DiffLineOrigin::Deletion => '-',
                };
                out.push(sigil);
                out.push_str(&line.content);
                out.push('\n');
                if !line.has_trailing_newline {
                    out.push_str("\\ No newline at end of file\n");
                }
            }
        }
    }
    out
}

fn patch_paths(file: &FileDiff) -> (String, String) {
    match file.change_type {
        ChangeType::Added => (
            "/dev/null".to_string(),
            format!("b/{}", file.path.to_string_lossy()),
        ),
        ChangeType::Deleted => (
            format!("a/{}", file.path.to_string_lossy()),
            "/dev/null".to_string(),
        ),
        _ => (
            format!(
                "a/{}",
                file.previous_path
                    .as_ref()
                    .unwrap_or(&file.path)
                    .to_string_lossy()
            ),
            format!("b/{}", file.path.to_string_lossy()),
        ),
    }
}

/// The result of [`export_patch`]: the rendered text plus enough scope
/// metadata for a presentation layer to tell a person exactly what the
/// patch does and does not cover (US-029 criterion 1: "origem e escopo do
/// patch são informados").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchExport {
    /// The rendered `git apply`-compatible patch text for every file in
    /// [`Self::included_files`]. Empty when nothing was renderable.
    pub patch: String,
    /// Files whose content hunks are included in [`Self::patch`].
    pub included_files: Vec<PathBuf>,
    /// Files omitted because they are binary (Diff & Blame Semantics Rules
    /// #3: a binary change is never turned into fabricated textual hunks,
    /// so it has nothing this renderer can express).
    pub skipped_binary_files: Vec<PathBuf>,
    /// Files omitted because their diff was too large and `hunks` was
    /// withheld by the adapter ([`FileDiff::truncated`]) — the change is
    /// real, its content is just unavailable at this size (Diff & Blame
    /// Semantics Rules #5), so this is reported distinctly from a binary
    /// skip rather than silently shrinking the patch's scope.
    pub skipped_truncated_files: Vec<PathBuf>,
}

impl PatchExport {
    /// Whether the rendered patch has no content at all — every input file
    /// was binary, truncated, or had no content hunks (e.g. a pure rename).
    /// A presentation layer must never offer to copy/save an empty patch as
    /// if it were a real export.
    pub fn is_empty(&self) -> bool {
        self.patch.is_empty()
    }

    /// Whether the export omitted part of what was asked for — a
    /// presentation layer must surface this rather than let a truncated or
    /// partly-binary export look complete (US-029 criterion 1).
    pub fn is_incomplete(&self) -> bool {
        !self.skipped_binary_files.is_empty() || !self.skipped_truncated_files.is_empty()
    }
}

/// Builds the full, `git apply`-compatible patch for every file in `files`
/// (US-029: "obter um patch das mudanças escolhidas"), reusing
/// [`render_unified_diff`] rather than a second, bespoke renderer.
///
/// Never mutates anything — `files` are read-only [`FileDiff`] values a
/// caller already obtained from [`crate::ports::RepositoryReadPort::diff`]
/// (US-029 criterion 2: exporting a patch never touches the repository).
/// Binary and truncated files are classified and excluded rather than
/// silently dropped, so a caller can tell a person exactly what is and is
/// not covered by the resulting patch.
pub fn export_patch(files: &[FileDiff]) -> PatchExport {
    let mut included_files = Vec::new();
    let mut skipped_binary_files = Vec::new();
    let mut skipped_truncated_files = Vec::new();

    for file in files {
        if file.is_binary {
            skipped_binary_files.push(file.path.clone());
        } else if file.truncated {
            skipped_truncated_files.push(file.path.clone());
        } else if !file.hunks.is_empty() {
            included_files.push(file.path.clone());
        }
    }

    PatchExport {
        patch: render_unified_diff(files),
        included_files,
        skipped_binary_files,
        skipped_truncated_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{DiffHunk, DiffLine};

    fn modified_file(path: &str) -> FileDiff {
        FileDiff {
            path: PathBuf::from(path),
            previous_path: None,
            change_type: ChangeType::Modified,
            is_binary: false,
            truncated: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![
                    DiffLine {
                        origin: DiffLineOrigin::Deletion,
                        content: "old".to_string(),
                        has_trailing_newline: true,
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Addition,
                        content: "new".to_string(),
                        has_trailing_newline: true,
                    },
                ],
            }],
        }
    }

    #[test]
    fn export_includes_a_plain_modified_file_and_renders_its_patch() {
        let export = export_patch(&[modified_file("a.txt")]);

        assert_eq!(export.included_files, vec![PathBuf::from("a.txt")]);
        assert!(export.skipped_binary_files.is_empty());
        assert!(export.skipped_truncated_files.is_empty());
        assert!(!export.is_empty());
        assert!(!export.is_incomplete());
        assert_eq!(
            export.patch,
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n"
        );
    }

    #[test]
    fn export_classifies_a_binary_file_as_skipped_rather_than_rendering_fabricated_hunks() {
        let file = FileDiff {
            path: PathBuf::from("image.png"),
            previous_path: None,
            change_type: ChangeType::Modified,
            is_binary: true,
            truncated: false,
            hunks: Vec::new(),
        };

        let export = export_patch(&[file]);

        assert!(export.included_files.is_empty());
        assert_eq!(
            export.skipped_binary_files,
            vec![PathBuf::from("image.png")]
        );
        assert!(export.is_empty());
        assert!(export.is_incomplete());
    }

    #[test]
    fn export_classifies_a_truncated_file_as_skipped_and_marks_the_export_incomplete() {
        let file = FileDiff {
            path: PathBuf::from("huge.txt"),
            previous_path: None,
            change_type: ChangeType::Modified,
            is_binary: false,
            truncated: true,
            hunks: Vec::new(),
        };

        let export = export_patch(&[file]);

        assert!(export.included_files.is_empty());
        assert_eq!(
            export.skipped_truncated_files,
            vec![PathBuf::from("huge.txt")]
        );
        assert!(export.is_incomplete());
    }

    #[test]
    fn export_across_multiple_files_reports_each_scope_bucket_independently() {
        let files = vec![
            modified_file("included.txt"),
            FileDiff {
                path: PathBuf::from("bin.dat"),
                previous_path: None,
                change_type: ChangeType::Modified,
                is_binary: true,
                truncated: false,
                hunks: Vec::new(),
            },
        ];

        let export = export_patch(&files);

        assert_eq!(export.included_files, vec![PathBuf::from("included.txt")]);
        assert_eq!(export.skipped_binary_files, vec![PathBuf::from("bin.dat")]);
        assert!(export.patch.contains("included.txt"));
        assert!(!export.patch.contains("bin.dat"));
    }

    #[test]
    fn export_of_no_files_is_empty_and_not_incomplete() {
        let export = export_patch(&[]);

        assert!(export.is_empty());
        assert!(!export.is_incomplete());
    }
}
