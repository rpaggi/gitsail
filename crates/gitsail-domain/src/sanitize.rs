//! Terminal-escape sanitization for repository-sourced text (SAD §33, EPIC-22/
//! US-110 criterion 2: "all repository text displayed in terminal or GUI is
//! treated as untrusted input... terminal escape/control sequences must be
//! sanitized before rendering").
//!
//! File names, branch names, commit messages, and diff/blame content all
//! originate from the repository and may be rendered verbatim by a
//! presentation layer — nothing stops a crafted commit message or file name
//! containing e.g. an ANSI escape sequence from reaching a terminal or a
//! GUI's DOM. [`safe_line`]/[`safe_path`] are the single, centralized point
//! every renderer (CLI, TUI, Desktop, VS Code via the protocol) should pass
//! such text through before display.
//!
//! This module is deliberately presentation-agnostic and lives in
//! `gitsail-domain` (no infra/UI dependency) precisely so it is not
//! reimplemented per interface (US-110 criterion 3). Domain/application code
//! keeps the raw value internally (for correct equality/lookup/hashing, and
//! so the original data is never lost) — only the render boundary sanitizes
//! (US-110 DoD: "só a apresentação é sanitizada, não o dado em si").
//!
//! Originally introduced in `gitsail-tui::sanitize` (EPIC-09); moved here
//! under EPIC-22/T-221 so `gitsail-cli`, `gitsail-tui`, and any future
//! protocol-facing renderer share exactly one implementation instead of each
//! reinventing it. `gitsail-tui::sanitize` now re-exports these functions
//! rather than defining its own.

use std::path::Path;

/// Replaces every control character (including ESC, and any other
/// `char::is_control` code point) with the Unicode replacement character,
/// preserving the string's character count so column alignment in a
/// rendered diff/blame line is not disturbed.
pub fn safe_line(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

/// [`safe_line`] applied to a path's display form.
pub fn safe_path(path: &Path) -> String {
    safe_line(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn control_characters_are_replaced_without_changing_length() {
        let input = "\u{1b}[31mred\u{1b}[0m";
        let sanitized = safe_line(input);
        assert!(!sanitized.contains('\u{1b}'), "escape byte must be gone");
        assert_eq!(sanitized.chars().count(), input.chars().count());
        assert!(sanitized.contains("red"), "surrounding text is preserved");
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(safe_line("hello world"), "hello world");
    }

    #[test]
    fn path_sanitization_reaches_through_to_safe_line() {
        let path = PathBuf::from("a\u{1b}[31mb");
        assert!(!safe_path(&path).contains('\u{1b}'));
    }

    /// EPIC-22/T-221 DoD fixture: a commit message carrying an OSC-8
    /// hyperlink-style escape sequence and a bell character (both control
    /// bytes) must have those bytes stripped from the *displayed* line while
    /// the original string (what a caller stores/compares) stays untouched.
    #[test]
    fn malicious_commit_message_with_ansi_and_bell_is_sanitized_for_display_only() {
        let malicious_message =
            "fix: \u{1b}]8;;file:///etc/passwd\u{7}click me\u{1b}]8;;\u{7} done";
        let displayed = safe_line(malicious_message);

        assert!(!displayed.contains('\u{1b}'));
        assert!(!displayed.contains('\u{7}'));
        assert!(displayed.contains("click me"));
        assert!(displayed.contains("done"));
        // The original value is never mutated by sanitizing a copy of it.
        assert!(malicious_message.contains('\u{1b}'));
    }

    /// A branch name containing a raw control character (reachable via
    /// plumbing even though `git branch` itself rejects most of these
    /// through porcelain) must sanitize the same way as any other line.
    #[test]
    fn malicious_branch_name_control_sequence_is_sanitized_for_display_only() {
        let malicious_name = "feature/\u{1b}[2Jclear-screen";
        let displayed = safe_line(malicious_name);
        assert!(!displayed.contains('\u{1b}'));
        assert!(displayed.contains("feature/"));
        assert!(displayed.contains("clear-screen"));
    }
}
