//! Terminal-escape sanitization for repository-sourced text (SAD §33: "todo
//! texto de repositório exibido é não confiável; sequências de escape/
//! controle do terminal devem ser sanitizadas antes de renderizar").
//!
//! File names, branch names, and diff/blame content all originate from the
//! repository and are rendered verbatim by `ui.rs` today — nothing stops a
//! crafted file name containing e.g. an ANSI escape sequence from reaching
//! the terminal. [`safe_line`]/[`safe_path`] are the single point every
//! renderer must pass such text through; `App` itself keeps the raw value
//! (for correct equality/lookup), only `ui` sanitizes at the render
//! boundary.

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
}
