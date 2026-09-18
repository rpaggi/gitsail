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

    // -- US-046: status/diff/blame inspection --------------------------
    /// Toggles the Diff panel between its diff and blame sub-views.
    ToggleBlameView,

    // -- US-048: branch administration ----------------------------------
    /// Opens the new-branch name prompt (`n`, Sidebar only).
    StartCreateBranch,
    /// Appends one character to the branch-name prompt.
    BranchNameInput(char),
    /// Removes the last character from the branch-name prompt.
    BranchNameBackspace,
    /// Requests a confirmation to check out the highlighted branch (`c`).
    RequestCheckout,
    /// Requests a confirmation to delete the highlighted branch (`d`).
    RequestDeleteBranch,

    // -- US-047: stage/unstage/commit -----------------------------------
    /// Stages or unstages the status entry under the cursor, depending on
    /// its scope (`s`, Details only).
    ToggleStage,
    /// Opens the commit-message composer (`C`).
    StartCommit,
    /// Appends one character to the commit message.
    CommitMessageInput(char),
    /// Removes the last character from the commit message.
    CommitMessageBackspace,

    // -- US-045: explore history and details -----------------------------
    /// Appends one character to the active commit-search box (`/` while
    /// the Graph panel is focused, routed by [`crate::app::App::update`]
    /// the same way [`Action::StartCreateBranch`] is gated to the Sidebar).
    CommitSearchInput(char),
    /// Removes the last character from the commit-search box.
    CommitSearchBackspace,
    /// Submits the commit-search box, replacing the loaded commit graph
    /// with a freshly filtered page (criterion 2) — parsed into
    /// [`gitsail_application::CommitQuery`] filters by
    /// [`crate::commit_search::parse_commit_search`], never a TUI-only
    /// text match.
    CommitSearchSubmit,

    // -- US-029: copy or export a patch ----------------------------------
    /// Copies the currently displayed diff's patch to the system clipboard
    /// (`y`, Diff panel only), falling back to saving a file when the
    /// clipboard is unavailable (US-029 criterion 3).
    ExportPatch,
}
