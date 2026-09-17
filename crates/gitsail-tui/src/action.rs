//! Typed actions produced from terminal input (SAD §18's "Terminal Events
//! -> Action -> ..."; US-041 criterion 1).
//!
//! [`crate::keymap::action_for`] is the only place that turns a raw key
//! press into an [`Action`]; [`crate::app::App::update`] is the only place
//! that turns an `Action` into a state change. Neither one knows about the
//! other's concern, so a keymap change can never accidentally alter
//! behavior and a behavior change can never accidentally alter a shortcut.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Moves focus to the next panel (US-042 criterion 2).
    FocusNext,
    /// Moves focus to the previous panel (US-042 criterion 2).
    FocusPrev,
    /// Moves the selection cursor up within the focused panel.
    MoveUp,
    /// Moves the selection cursor down within the focused panel.
    MoveDown,
    /// Activates the highlighted item (Enter).
    Activate,
    /// Closes whatever overlay/mode is on top (help, search, ...) without
    /// side effects.
    Dismiss,
    /// Toggles the contextual help overlay (US-042 criterion 3).
    ToggleHelp,
    /// Enters branch-filter search mode (`/`).
    StartSearch,
    /// Appends one character to the active search query.
    SearchInput(char),
    /// Removes the last character from the active search query.
    SearchBackspace,
    /// Requests a manual status/branches refresh (SAD §22).
    Refresh,
    /// Requests a clean shutdown.
    Quit,
}
