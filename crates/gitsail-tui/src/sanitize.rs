//! Terminal-escape sanitization for repository-sourced text (SAD §33).
//!
//! Moved to `gitsail_domain::sanitize` under EPIC-22/T-221 so this logic is
//! centralized in exactly one place instead of being reimplemented per
//! presentation layer (US-110 criterion 3) — this module now re-exports
//! that implementation rather than defining its own, keeping every existing
//! call site in `ui.rs`/`graph_view.rs` (which refer to `sanitize::safe_line`
//! / `sanitize::safe_path`) unchanged.

pub use gitsail_domain::sanitize::{safe_line, safe_path};

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
}
