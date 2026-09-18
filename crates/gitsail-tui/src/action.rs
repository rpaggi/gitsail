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
    /// Opens the rename prompt for the highlighted branch (`R`, Sidebar
    /// only), pre-filled with its current name (T-157/US-024). Unlike
    /// [`Action::RequestDeleteBranch`], this is never a no-op on the
    /// current branch — renaming the branch a person is standing on is
    /// exactly as valid as renaming any other local branch.
    StartRenameBranch,

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

    // -- US-049: sync with a remote --------------------------------------
    /// Fetches the resolved remote (`f`). `Safe`, so this dispatches
    /// immediately rather than confirming first, exactly like
    /// [`Action::ToggleStage`].
    RequestFetch,
    /// Requests confirmation to pull the resolved remote's tracked branch
    /// (`p`) — fast-forward only; see [`crate::operation::OperationKind::Pull`].
    RequestPull,
    /// Requests confirmation to push the current branch to the resolved
    /// remote (`P`).
    RequestPush,

    // -- US-050: inspect tags, remotes and stash --------------------------
    /// Cycles the References panel between its Tags/Remotes/Stash sub-views
    /// (`t`, References panel only), mirroring [`Action::ToggleBlameView`].
    CycleReferenceView,

    // -- T-163/US-030: apply a patch --------------------------------------
    /// Requests applying the patch currently on the system clipboard (`Y`,
    /// Diff panel only — the inverse of [`Action::ExportPatch`]'s `y`).
    /// Never applies immediately: this only starts the non-mutating
    /// preview (`git apply --check`); confirming the resulting prompt is
    /// what actually dispatches [`crate::worker::Command::ApplyPatch`]
    /// (US-030 criterion 1).
    RequestApplyPatch,

    // -- EPIC-16/T-231..T-233: merge, conflicts, continue/abort -----------
    /// Requests confirmation to merge the highlighted reference into the
    /// current branch (`m`, Sidebar only — reuses the same branch
    /// search/selection mechanism [`Action::RequestCheckout`] already uses;
    /// T-231/US-079 criterion 1: origin, destination and policy are shown
    /// before executing).
    RequestMerge,
    /// Opens or closes the conflicts overlay (`M`) — a no-op when no
    /// operation with conflicts is currently pending (T-232/US-080
    /// criterion 1).
    ToggleConflictsPanel,
    /// Loads the base/ours/theirs sides of the conflicted file under the
    /// conflicts overlay's cursor (Enter, T-232/US-080 criterion 2).
    InspectConflict,
    /// Marks the conflicted file under the cursor resolved by staging its
    /// current working-tree content (`r`, T-232/US-080 criterion 3) — only
    /// ever this explicit action, never inferred.
    MarkConflictResolved,
    /// Resolves the conflicted file under the cursor by taking "ours"
    /// wholesale (`o`) — the documented binary-conflict flow (T-232/US-080
    /// criterion 3), equally usable for a text file.
    TakeConflictSideOurs,
    /// Resolves the conflicted file under the cursor by taking "theirs"
    /// wholesale (`t` within the conflicts overlay).
    TakeConflictSideTheirs,
    /// Requests confirmation to continue the pending operation (`c` within
    /// the conflicts overlay, T-233/US-081) — only offered when
    /// [`gitsail_domain::OperationCapability::Continue`] is supported by
    /// whatever is currently detected.
    RequestContinueOperation,
    /// Requests confirmation to abort the pending operation (`a` within the
    /// conflicts overlay, T-233/US-081) — only offered when
    /// [`gitsail_domain::OperationCapability::Abort`] is supported.
    RequestAbortOperation,

    // -- EPIC-17/T-235: rebase, skip -------------------------------------
    /// Requests confirmation to rebase the current branch onto the
    /// highlighted reference (`o`, Sidebar only — reuses the same branch
    /// search/selection mechanism [`Action::RequestMerge`] already uses;
    /// T-235/US-083 criterion 1: current branch, chosen base and expected
    /// rewrite are shown before executing).
    RequestRebase,
    /// Requests confirmation to skip the current step of the pending
    /// operation (`s` within the conflicts overlay, T-235/US-083 criterion
    /// 3) — only offered when
    /// [`gitsail_domain::OperationCapability::Skip`] is supported (a merge
    /// never offers it).
    RequestSkipOperation,

    // -- T-236/US-084: plan an interactive rebase -------------------------
    /// Opens the interactive rebase plan overlay for the highlighted
    /// reference (`O`, Sidebar only — the capital counterpart of
    /// [`Action::RequestRebase`]'s lowercase `o`, reusing the same branch
    /// search/selection mechanism). Dispatches
    /// [`crate::worker::Command::PlanRebase`] immediately: reading a plan
    /// never touches the working tree, the index, or any ref, so there is
    /// nothing to confirm yet (T-236/US-084 criterion 1).
    RequestRebasePlan,
    /// Moves the highlighted plan entry one position up (`K` within
    /// [`crate::keymap::InputContext::RebasePlan`] — reordering, distinct
    /// from the plain cursor movement `k`/[`Action::MoveUp`] already does).
    RebasePlanMoveEntryUp,
    /// Moves the highlighted plan entry one position down (`J`), mirroring
    /// [`Action::RebasePlanMoveEntryUp`].
    RebasePlanMoveEntryDown,
    /// Cycles the highlighted entry's action Pick -> Reword -> Squash ->
    /// Fixup -> Drop -> Pick (`a` within
    /// [`crate::keymap::InputContext::RebasePlan`]).
    RebasePlanCycleAction,
    /// Appends one character to the Reword message prompt.
    RebasePlanRewordInput(char),
    /// Removes the last character from the Reword message prompt.
    RebasePlanRewordBackspace,
}
