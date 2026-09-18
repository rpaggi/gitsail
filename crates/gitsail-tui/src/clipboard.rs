//! System clipboard access for patch export (US-029/T-162, criterion 3).
//!
//! Kept as its own narrow port + adapter, distinct from
//! [`gitsail_application::RepositoryReadPort`]/
//! [`gitsail_application::RepositoryWritePort`]: clipboard access has no
//! Git semantics and is not something a CLI or Desktop consumer of this
//! crate's core would ever need, so it stays a presentation-only
//! dependency rather than living in `gitsail-application` (keeping the
//! domain/application layers free of any presentation concern, per
//! `AGENTS.md`'s dependency-direction rule). Defining the trait here — with
//! [`SystemClipboard`] as the one real adapter — also lets [`crate::app::App`]
//! be exercised in tests against [`FakeClipboard`] instead of a real OS
//! clipboard, which is routinely unavailable in a headless/CI/SSH
//! environment: exactly the case criterion 3 requires a documented
//! file-based fallback for.

/// Presentation-only capability to replace the system clipboard's
/// contents. `Send + Sync` so it can be held the same way [`std::sync::Arc`]
/// already holds [`gitsail_application::RepositoryReadPort`] in
/// [`crate::app::App`].
pub trait ClipboardPort: Send + Sync {
    /// Attempts to replace the system clipboard's contents with `text`.
    /// Returns a short, human-readable reason on failure (e.g. "no display
    /// server available") rather than panicking — a failure here must
    /// always fall back to saving a file (criterion 3), never lose the
    /// patch or crash the TUI.
    fn set_text(&self, text: &str) -> Result<(), String>;

    /// Attempts to read the system clipboard's current text contents
    /// (T-163/US-030: applying a patch currently on the clipboard is the
    /// inverse of [`Self::set_text`]'s export). Returns a short,
    /// human-readable reason on failure (no display server, clipboard
    /// empty, or clipboard holding non-text data) rather than panicking.
    fn get_text(&self) -> Result<String, String>;
}

/// Real clipboard access via `arboard` (see `Cargo.toml` for why this crate
/// was chosen). Constructs a fresh [`arboard::Clipboard`] per call rather
/// than holding one open: a held clipboard handle can go stale if the
/// display server restarts under a long-running TUI session, and opening
/// one is cheap relative to a keypress-driven action.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboard;

impl ClipboardPort for SystemClipboard {
    fn set_text(&self, text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text.to_string()).map_err(|e| e.to_string())
    }

    fn get_text(&self) -> Result<String, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.get_text().map_err(|e| e.to_string())
    }
}

/// A [`ClipboardPort`] test double, used by both this crate's own unit
/// tests ([`crate::app`]) and its `tests/` integration suite to exercise
/// the copy-succeeds and clipboard-unavailable-falls-back-to-file paths
/// deterministically, without depending on a real display server (which a
/// CI runner or SSH session commonly lacks).
#[derive(Debug, Default)]
pub struct FakeClipboard {
    /// When `true`, [`Self::set_text`] always fails, simulating an
    /// unavailable clipboard (criterion 3).
    pub fail: bool,
    /// The last text successfully handed to [`Self::set_text`], for
    /// assertions.
    pub last_set: std::sync::Mutex<Option<String>>,
    /// What [`Self::get_text`] returns on success (T-163/US-030). `None`
    /// (the default) simulates an empty/unavailable clipboard, returning
    /// `Err` the same way a real empty clipboard would for `arboard`.
    pub contents: std::sync::Mutex<Option<String>>,
}

impl ClipboardPort for FakeClipboard {
    fn set_text(&self, text: &str) -> Result<(), String> {
        if self.fail {
            return Err("no display server available (fake)".to_string());
        }
        *self.last_set.lock().unwrap() = Some(text.to_string());
        Ok(())
    }

    fn get_text(&self) -> Result<String, String> {
        if self.fail {
            return Err("no display server available (fake)".to_string());
        }
        self.contents
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "clipboard is empty (fake)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clipboard_records_the_last_successful_write() {
        let clipboard = FakeClipboard::default();
        clipboard.set_text("hello").unwrap();
        assert_eq!(clipboard.last_set.lock().unwrap().as_deref(), Some("hello"));
    }

    #[test]
    fn fake_clipboard_reports_failure_without_recording_anything() {
        let clipboard = FakeClipboard {
            fail: true,
            ..Default::default()
        };
        assert!(clipboard.set_text("hello").is_err());
        assert!(clipboard.last_set.lock().unwrap().is_none());
    }

    #[test]
    fn fake_clipboard_get_text_returns_configured_contents() {
        let clipboard = FakeClipboard {
            contents: std::sync::Mutex::new(Some("a patch".to_string())),
            ..Default::default()
        };
        assert_eq!(clipboard.get_text().unwrap(), "a patch");
    }

    #[test]
    fn fake_clipboard_get_text_fails_when_empty_or_unavailable() {
        let empty = FakeClipboard::default();
        assert!(empty.get_text().is_err());

        let unavailable = FakeClipboard {
            fail: true,
            contents: std::sync::Mutex::new(Some("ignored".to_string())),
            ..Default::default()
        };
        assert!(unavailable.get_text().is_err());
    }
}
