//! Key press -> [`Action`] mapping, gated by [`InputContext`] (US-042
//! criterion 1: "Setas/j/k, Enter, /, q e ? seguem contexto documentado";
//! criterion 3 / DoD: a key that has no meaning in the current context
//! never falls through to an action from a different one — e.g. `q` closes
//! the help overlay instead of quitting while help is open).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;

/// What the keyboard means right now. Computed by [`crate::app::App`] from
/// its own state ([`crate::app::App::input_context`]) so this module never
/// needs to know *why* a context applies, only which keys are meaningful
/// in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    /// The contextual help overlay is open (US-042 criterion 3).
    Help,
    /// Branch-filter search input is active.
    Search,
    /// The new-branch name prompt is active (US-048).
    BranchName,
    /// The rename-branch prompt is active, pre-filled with the branch's
    /// previous name (T-157/US-024). Routes identically to
    /// [`Self::BranchName`] here — [`crate::app::App`] is what tells the
    /// two apart (whether a rename source is pending) and gives them
    /// distinct meaning on `Enter`.
    RenameBranch,
    /// The commit-message composer is active (US-047).
    CommitMessage,
    /// The commit-search box is active (`/` on the Graph panel, US-045
    /// criterion 2).
    CommitSearch,
    /// The commit-details overlay is open (Enter on the Graph panel,
    /// US-045 criterion 3).
    CommitDetails,
    /// The reference-details overlay is open (Enter on the References
    /// panel, US-050 criterion 2), mirroring [`Self::CommitDetails`].
    ReferenceDetails,
    /// The reflog-entry details overlay is open (Enter on the References
    /// panel while its Reflog sub-view is active, T-241/US-089 criterion
    /// 2), mirroring [`Self::CommitDetails`]/[`Self::ReferenceDetails`]
    /// exactly — dismiss-only, never a reset or any other mutation.
    ReflogDetails,
    /// The conflicts overlay is open (`M`, T-232/US-080/T-233/US-081) —
    /// unlike [`Self::CommitDetails`]/[`Self::ReferenceDetails`], this
    /// overlay is itself navigable (multiple conflicted files) and offers
    /// resolution/continue/abort actions, not just dismiss.
    Conflicts,
    /// The interactive rebase plan overlay is open (`O`, T-236/US-084) —
    /// navigable like [`Self::Conflicts`], but reordering/reassigning
    /// actions rather than resolving conflicts.
    RebasePlan,
    /// The Reword message prompt is active within the rebase plan overlay
    /// (T-236/US-084's "reaproveite o mecanismo de input de texto"), taking
    /// priority over [`Self::RebasePlan`] exactly like
    /// [`Self::CommitMessage`] takes priority over [`Self::Normal`].
    RebasePlanReword,
    /// The reset-mode chooser overlay is open (`z`, T-240/US-088) —
    /// navigable like [`Self::RebasePlan`], picking soft/mixed/hard before
    /// anything is confirmed.
    ResetMode,
    /// The amend composer is open (`A`, T-242/US-090), editing the message
    /// that will replace `HEAD`'s (US-090 criterion 1: reuses the same
    /// `gitsail_application::AmendCommit`/`PreviewAmend` use cases the
    /// Desktop already exercises, no parallel Git implementation). Stays
    /// active through `Confirming`/`InProgress`/`Failed` — mirrors
    /// [`Self::CommitMessage`]'s own "stays open through confirmation"
    /// convention (US-090 criterion 3: a failed amend must never lose the
    /// typed message), unlike [`Self::ResetMode`]/[`Self::RebasePlan`],
    /// which hand off to the generic operation overlay instead.
    Amend,
    /// No overlay is active; the panels and shortcuts bar are live.
    Normal,
}

/// Maps one key press to an [`Action`], or `None` when `key` has no
/// documented meaning in `ctx`.
pub fn action_for(key: KeyEvent, ctx: InputContext) -> Option<Action> {
    // Crossterm reports both press and release on platforms that support
    // it (e.g. Windows); only presses should ever trigger an action, or a
    // key held down would fire twice per press-release pair.
    if key.kind != KeyEventKind::Press {
        return None;
    }

    match ctx {
        InputContext::Help => match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => Some(Action::Dismiss),
            _ => None,
        },
        InputContext::Search => match key.code {
            KeyCode::Esc | KeyCode::Enter => Some(Action::Dismiss),
            KeyCode::Backspace => Some(Action::SearchBackspace),
            KeyCode::Char(c) => Some(Action::SearchInput(c)),
            _ => None,
        },
        InputContext::BranchName | InputContext::RenameBranch => match key.code {
            KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Enter => Some(Action::Activate),
            KeyCode::Backspace => Some(Action::BranchNameBackspace),
            KeyCode::Char(c) => Some(Action::BranchNameInput(c)),
            _ => None,
        },
        InputContext::CommitMessage => match key.code {
            KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Enter => Some(Action::Activate),
            KeyCode::Backspace => Some(Action::CommitMessageBackspace),
            KeyCode::Char(c) => Some(Action::CommitMessageInput(c)),
            _ => None,
        },
        InputContext::CommitSearch => match key.code {
            KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Enter => Some(Action::CommitSearchSubmit),
            KeyCode::Backspace => Some(Action::CommitSearchBackspace),
            KeyCode::Char(c) => Some(Action::CommitSearchInput(c)),
            _ => None,
        },
        InputContext::CommitDetails => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            _ => None,
        },
        InputContext::ReferenceDetails => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            _ => None,
        },
        InputContext::ReflogDetails => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            _ => None,
        },
        InputContext::Conflicts => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
            KeyCode::Enter => Some(Action::InspectConflict),
            KeyCode::Char('r') => Some(Action::MarkConflictResolved),
            KeyCode::Char('o') => Some(Action::TakeConflictSideOurs),
            KeyCode::Char('t') => Some(Action::TakeConflictSideTheirs),
            KeyCode::Char('c') => Some(Action::RequestContinueOperation),
            KeyCode::Char('a') => Some(Action::RequestAbortOperation),
            KeyCode::Char('s') => Some(Action::RequestSkipOperation),
            _ => None,
        },
        InputContext::RebasePlan => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
            KeyCode::Char('K') => Some(Action::RebasePlanMoveEntryUp),
            KeyCode::Char('J') => Some(Action::RebasePlanMoveEntryDown),
            KeyCode::Char('a') => Some(Action::RebasePlanCycleAction),
            KeyCode::Enter => Some(Action::Activate),
            _ => None,
        },
        InputContext::RebasePlanReword => match key.code {
            KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Enter => Some(Action::Activate),
            KeyCode::Backspace => Some(Action::RebasePlanRewordBackspace),
            KeyCode::Char(c) => Some(Action::RebasePlanRewordInput(c)),
            _ => None,
        },
        InputContext::ResetMode => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
            KeyCode::Enter => Some(Action::Activate),
            _ => None,
        },
        InputContext::Amend => match key.code {
            KeyCode::Esc => Some(Action::Dismiss),
            KeyCode::Enter => Some(Action::Activate),
            KeyCode::Backspace => Some(Action::AmendMessageBackspace),
            KeyCode::Char(c) => Some(Action::AmendMessageInput(c)),
            _ => None,
        },
        InputContext::Normal => match key.code {
            KeyCode::Tab => Some(Action::FocusNext),
            KeyCode::BackTab => Some(Action::FocusPrev),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
            KeyCode::Enter => Some(Action::Activate),
            KeyCode::Char('/') => Some(Action::StartSearch),
            KeyCode::Char('r') => Some(Action::Refresh),
            KeyCode::Char('?') => Some(Action::ToggleHelp),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('b') => Some(Action::ToggleBlameView),
            KeyCode::Char('n') => Some(Action::StartCreateBranch),
            KeyCode::Char('c') => Some(Action::RequestCheckout),
            KeyCode::Char('d') => Some(Action::RequestDeleteBranch),
            KeyCode::Char('R') => Some(Action::StartRenameBranch),
            KeyCode::Char('s') => Some(Action::ToggleStage),
            KeyCode::Char('C') => Some(Action::StartCommit),
            KeyCode::Char('y') => Some(Action::ExportPatch),
            KeyCode::Char('Y') => Some(Action::RequestApplyPatch),
            KeyCode::Char('f') => Some(Action::RequestFetch),
            KeyCode::Char('p') => Some(Action::RequestPull),
            KeyCode::Char('P') => Some(Action::RequestPush),
            KeyCode::Char('t') => Some(Action::CycleReferenceView),
            KeyCode::Char('m') => Some(Action::RequestMerge),
            KeyCode::Char('M') => Some(Action::ToggleConflictsPanel),
            KeyCode::Char('o') => Some(Action::RequestRebase),
            KeyCode::Char('O') => Some(Action::RequestRebasePlan),
            KeyCode::Char('x') => Some(Action::RequestCherryPick),
            KeyCode::Char('v') => Some(Action::RequestRevert),
            KeyCode::Char('z') => Some(Action::RequestReset),
            KeyCode::Char('A') => Some(Action::StartAmend),
            KeyCode::Char('w') => Some(Action::RequestOpenForgeLink),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn help_context_only_accepts_dismiss_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('q')), InputContext::Help),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('?')), InputContext::Help),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::Help),
            None,
            "movement must not leak through the help overlay"
        );
        assert_eq!(
            action_for(press(KeyCode::Tab), InputContext::Help),
            None,
            "focus change must not leak through the help overlay"
        );
    }

    #[test]
    fn search_context_routes_characters_and_control_keys_distinctly() {
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::Search),
            Some(Action::SearchInput('a'))
        );
        assert_eq!(
            action_for(press(KeyCode::Backspace), InputContext::Search),
            Some(Action::SearchBackspace)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::Search),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Tab), InputContext::Search),
            None,
            "focus change is not a documented search-mode shortcut"
        );
    }

    #[test]
    fn commit_search_context_routes_characters_and_submits_on_enter() {
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::CommitSearch),
            Some(Action::CommitSearchInput('a'))
        );
        assert_eq!(
            action_for(press(KeyCode::Backspace), InputContext::CommitSearch),
            Some(Action::CommitSearchBackspace)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::CommitSearch),
            Some(Action::CommitSearchSubmit)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::CommitSearch),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Tab), InputContext::CommitSearch),
            None,
            "focus change is not a documented commit-search shortcut"
        );
    }

    #[test]
    fn commit_details_context_only_accepts_dismiss_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('q')), InputContext::CommitDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::CommitDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::CommitDetails),
            None,
            "movement must not leak through the commit-details overlay"
        );
    }

    #[test]
    fn normal_context_maps_documented_shortcuts() {
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::Normal),
            Some(Action::MoveDown)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('k')), InputContext::Normal),
            Some(Action::MoveUp)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::Normal),
            Some(Action::Activate)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('/')), InputContext::Normal),
            Some(Action::StartSearch)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('q')), InputContext::Normal),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('?')), InputContext::Normal),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn y_maps_to_export_patch_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('y')), InputContext::Normal),
            Some(Action::ExportPatch)
        );
    }

    #[test]
    fn shift_y_maps_to_request_apply_patch_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('Y')), InputContext::Normal),
            Some(Action::RequestApplyPatch)
        );
    }

    #[test]
    fn remote_sync_and_reference_keys_map_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('f')), InputContext::Normal),
            Some(Action::RequestFetch)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('p')), InputContext::Normal),
            Some(Action::RequestPull)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('P')), InputContext::Normal),
            Some(Action::RequestPush)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('t')), InputContext::Normal),
            Some(Action::CycleReferenceView)
        );
    }

    #[test]
    fn reference_details_context_only_accepts_dismiss_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('q')), InputContext::ReferenceDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::ReferenceDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::ReferenceDetails),
            None,
            "movement must not leak through the reference-details overlay"
        );
    }

    #[test]
    fn shift_r_starts_rename_branch_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('R')), InputContext::Normal),
            Some(Action::StartRenameBranch)
        );
    }

    #[test]
    fn rename_branch_context_routes_exactly_like_branch_name() {
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::RenameBranch),
            Some(Action::BranchNameInput('a'))
        );
        assert_eq!(
            action_for(press(KeyCode::Backspace), InputContext::RenameBranch),
            Some(Action::BranchNameBackspace)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::RenameBranch),
            Some(Action::Activate)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::RenameBranch),
            Some(Action::Dismiss)
        );
    }

    #[test]
    fn merge_and_toggle_conflicts_keys_map_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('m')), InputContext::Normal),
            Some(Action::RequestMerge)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('M')), InputContext::Normal),
            Some(Action::ToggleConflictsPanel)
        );
    }

    /// T-235/US-083: `o` requests a rebase in the Normal context (reusing
    /// the same sidebar branch picker `m`/`RequestMerge` already uses), and
    /// `s` requests skip within the conflicts overlay.
    #[test]
    fn rebase_and_skip_keys_map_in_their_own_contexts() {
        assert_eq!(
            action_for(press(KeyCode::Char('o')), InputContext::Normal),
            Some(Action::RequestRebase)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('s')), InputContext::Conflicts),
            Some(Action::RequestSkipOperation)
        );
    }

    #[test]
    fn conflicts_context_routes_navigation_and_resolution_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::Conflicts),
            Some(Action::MoveDown)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('k')), InputContext::Conflicts),
            Some(Action::MoveUp)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::Conflicts),
            Some(Action::InspectConflict)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('r')), InputContext::Conflicts),
            Some(Action::MarkConflictResolved)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('o')), InputContext::Conflicts),
            Some(Action::TakeConflictSideOurs)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('t')), InputContext::Conflicts),
            Some(Action::TakeConflictSideTheirs)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('c')), InputContext::Conflicts),
            Some(Action::RequestContinueOperation)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::Conflicts),
            Some(Action::RequestAbortOperation)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::Conflicts),
            Some(Action::Dismiss)
        );
    }

    /// T-236/US-084: `O` opens the interactive rebase plan (distinct from
    /// lowercase `o`'s plain rebase), and the plan overlay's own keys route
    /// distinctly from every other context.
    #[test]
    fn shift_o_requests_a_rebase_plan_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('O')), InputContext::Normal),
            Some(Action::RequestRebasePlan)
        );
    }

    #[test]
    fn rebase_plan_context_routes_navigation_reorder_and_action_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::RebasePlan),
            Some(Action::MoveDown)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('k')), InputContext::RebasePlan),
            Some(Action::MoveUp)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('J')), InputContext::RebasePlan),
            Some(Action::RebasePlanMoveEntryDown)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('K')), InputContext::RebasePlan),
            Some(Action::RebasePlanMoveEntryUp)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::RebasePlan),
            Some(Action::RebasePlanCycleAction)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::RebasePlan),
            Some(Action::Activate)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::RebasePlan),
            Some(Action::Dismiss)
        );
    }

    #[test]
    fn rebase_plan_reword_context_routes_characters_and_confirms_on_enter() {
        assert_eq!(
            action_for(press(KeyCode::Char('a')), InputContext::RebasePlanReword),
            Some(Action::RebasePlanRewordInput('a'))
        );
        assert_eq!(
            action_for(press(KeyCode::Backspace), InputContext::RebasePlanReword),
            Some(Action::RebasePlanRewordBackspace)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::RebasePlanReword),
            Some(Action::Activate)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::RebasePlanReword),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Tab), InputContext::RebasePlanReword),
            None,
            "focus change is not a documented reword-prompt shortcut"
        );
    }

    /// T-241/US-089: the reflog-details overlay only accepts dismiss keys,
    /// mirroring [`InputContext::ReferenceDetails`]/[`InputContext::CommitDetails`]
    /// exactly.
    #[test]
    fn reflog_details_context_only_accepts_dismiss_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('q')), InputContext::ReflogDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::ReflogDetails),
            Some(Action::Dismiss)
        );
        assert_eq!(
            action_for(press(KeyCode::Char('j')), InputContext::ReflogDetails),
            None,
            "movement must not leak through the reflog-details overlay"
        );
    }

    /// T-242/US-090: `A` opens the amend composer in the Normal context, and
    /// the composer routes characters/backspace/confirm/cancel exactly like
    /// [`InputContext::CommitMessage`].
    #[test]
    fn shift_a_starts_amend_in_the_normal_context() {
        assert_eq!(
            action_for(press(KeyCode::Char('A')), InputContext::Normal),
            Some(Action::StartAmend)
        );
    }

    #[test]
    fn amend_context_routes_characters_and_confirms_on_enter() {
        assert_eq!(
            action_for(press(KeyCode::Char('x')), InputContext::Amend),
            Some(Action::AmendMessageInput('x'))
        );
        assert_eq!(
            action_for(press(KeyCode::Backspace), InputContext::Amend),
            Some(Action::AmendMessageBackspace)
        );
        assert_eq!(
            action_for(press(KeyCode::Enter), InputContext::Amend),
            Some(Action::Activate)
        );
        assert_eq!(
            action_for(press(KeyCode::Esc), InputContext::Amend),
            Some(Action::Dismiss)
        );
    }

    #[test]
    fn only_key_press_events_produce_actions() {
        let mut key = press(KeyCode::Char('q'));
        key.kind = KeyEventKind::Release;
        assert_eq!(action_for(key, InputContext::Normal), None);
    }
}
