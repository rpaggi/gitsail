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
    /// The commit-message composer is active (US-047).
    CommitMessage,
    /// No overlay is active; the five panels and shortcuts bar are live.
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
        InputContext::BranchName => match key.code {
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
            KeyCode::Char('s') => Some(Action::ToggleStage),
            KeyCode::Char('C') => Some(Action::StartCommit),
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
    fn only_key_press_events_produce_actions() {
        let mut key = press(KeyCode::Char('q'));
        key.kind = KeyEventKind::Release;
        assert_eq!(action_for(key, InputContext::Normal), None);
    }
}
