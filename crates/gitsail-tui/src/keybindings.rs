//! Documented keyboard remapping for the TUI (T-251/US-109 criterion 2:
//! "TUI oferece remapeamento documentado sem exigir perfis Vim/Emacs").
//!
//! This is deliberately **not** a general "rebind anything" facility, and
//! it never grows into a Vim/Emacs mode emulator — both are explicitly out
//! of scope per this story. Instead it mirrors the shape
//! `apps/desktop/src/keybindings.ts` (T-249/US-107) already established:
//! a small, fixed, documented **allow-list** of common commands
//! ([`CONFIGURABLE_ACTIONS`]), each with a single-character default key a
//! person may override via a plain-text config file.
//!
//! **Why this is an allow-list, not a raw key -> [`Action`] table (the
//! load-bearing safety property this module exists to guarantee, T-251
//! criterion 3):** [`Action::Activate`] and [`Action::Dismiss`] — the two
//! actions that drive [`crate::operation::OperationState::confirm`]/
//! [`crate::operation::OperationState::cancel`] for a pending `Moderate`/
//! `Destructive` confirmation — are never listed in
//! [`CONFIGURABLE_ACTIONS`]. There is therefore no config line, valid or
//! malformed, that can ever produce an override for either of them:
//! [`parse_config`] resolves every `action = key` line strictly by looking
//! up `action` in this fixed registry, so an id that is not here (whether
//! it is a typo, or a deliberate attempt to name `"activate"`/`"dismiss"`)
//! is rejected as an unknown action, exactly like any other typo, and
//! never silently accepted. Nor can two keys ever be merged into an
//! existing action's meaning: [`effective_bindings`] only ever substitutes
//! *which key* triggers one of this registry's already-fixed
//! [`Action`] values, it never invents a new one. Combined with
//! [`crate::keymap::resolve_action`] only ever consulting these bindings
//! while [`crate::keymap::InputContext::Normal`] is active (every overlay,
//! prompt, and dialog — including the confirmation prompt itself — always
//! resolves through the original, hardcoded [`crate::keymap::action_for`]
//! alone), a remapped key can change *what starts* a mutation, but can
//! never change how a pending confirmation is advanced or cancelled. See
//! this module's own tests, and
//! `crate::app::tests::a_destructive_reset_still_requires_two_explicit_confirmations`,
//! for the executable proof.
//!
//! **Format**: plain text, one `action-id = X` assignment per line, where
//! `X` is exactly one character (this registry's shortcuts are all
//! single, unmodified characters, unlike Desktop's `Mod+...` chords — the
//! TUI's own established convention, see `crate::keymap`). Blank lines and
//! lines starting with `#` are ignored. An unknown action id, or a value
//! that is not exactly one character, is reported as a warning and
//! ignored — never a startup failure (mirrors `gitsail_application::
//! preferences`'s own "an invalid preferences file falls back to safe
//! defaults and reports the problem" rule).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::action::Action;

/// One command this TUI lets a person rebind (T-251 criterion 2). Naming a
/// real, already-existing shortcut (never an aspirational one) mirrors
/// `apps/desktop/src/keybindings.ts::CONFIGURABLE_ACTIONS`'s own
/// convention.
#[derive(Debug, Clone, Copy)]
pub struct ConfigurableAction {
    /// Stable identifier used in the config file, kebab-case to match
    /// Desktop's own id style (`"focus-search"`, `"fetch"`, ...) — the two
    /// interfaces document the same *shape* of remapping even though their
    /// storage/config formats are necessarily distinct (see
    /// `docs/architecture/preferences-matrix.md`).
    pub id: &'static str,
    /// Human-readable label, for a future settings/help surface.
    pub label: &'static str,
    pub action: Action,
    pub default_key: char,
}

/// The fixed, documented allow-list (T-251 criterion 2's "pelo menos as
/// ações mais comuns"). Every entry here is a [`crate::keymap::
/// InputContext::Normal`]-only command with no confirmation semantics of
/// its own attached to the *keypress* — opening the reset-mode chooser or
/// the amend composer, for instance, is not itself a mutation (see their
/// own doc comments in `crate::app`), so remapping which key opens them
/// changes nothing about how the mutation behind them is later confirmed.
///
/// Deliberately excluded, permanently: [`Action::Activate`],
/// [`Action::Dismiss`], [`Action::MoveUp`], [`Action::MoveDown`],
/// [`Action::FocusNext`], [`Action::FocusPrev`] — the structural
/// navigation/confirmation primitives every overlay in this crate is built
/// on. See this module's own doc comment for why leaving out the first two
/// specifically is the actual safety guarantee T-251 criterion 3 requires.
pub const CONFIGURABLE_ACTIONS: &[ConfigurableAction] = &[
    ConfigurableAction { id: "quit", label: "Quit", action: Action::Quit, default_key: 'q' },
    ConfigurableAction {
        id: "toggle-help",
        label: "Toggle help",
        action: Action::ToggleHelp,
        default_key: '?',
    },
    ConfigurableAction { id: "refresh", label: "Refresh", action: Action::Refresh, default_key: 'r' },
    ConfigurableAction {
        id: "toggle-stage",
        label: "Stage/unstage",
        action: Action::ToggleStage,
        default_key: 's',
    },
    ConfigurableAction {
        id: "start-commit",
        label: "Compose a commit",
        action: Action::StartCommit,
        default_key: 'C',
    },
    ConfigurableAction {
        id: "request-fetch",
        label: "Fetch",
        action: Action::RequestFetch,
        default_key: 'f',
    },
    ConfigurableAction {
        id: "request-pull",
        label: "Pull",
        action: Action::RequestPull,
        default_key: 'p',
    },
    ConfigurableAction {
        id: "request-push",
        label: "Push",
        action: Action::RequestPush,
        default_key: 'P',
    },
    ConfigurableAction {
        id: "toggle-blame-view",
        label: "Toggle blame view",
        action: Action::ToggleBlameView,
        default_key: 'b',
    },
    ConfigurableAction {
        id: "start-search",
        label: "Search",
        action: Action::StartSearch,
        default_key: '/',
    },
];

/// The result of [`parse_config`]: overrides that were actually accepted,
/// plus a human-readable warning for every rejected line — never a hard
/// parse failure (mirrors `gitsail_application::preferences`'s own
/// "invalid input degrades to safe defaults, never crashes" convention).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedConfig {
    pub overrides: HashMap<String, char>,
    pub warnings: Vec<String>,
}

/// Parses the keybindings-override config format (this module's own doc
/// comment). Every accepted key in [`ParsedConfig::overrides`] is
/// guaranteed to be one of [`CONFIGURABLE_ACTIONS`]'s own `id`s — this is
/// what makes an override for `"activate"`/`"dismiss"` structurally
/// impossible rather than merely undocumented (T-251 criterion 3).
pub fn parse_config(source: &str) -> ParsedConfig {
    let mut overrides = HashMap::new();
    let mut warnings = Vec::new();

    for (index, raw_line) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((id_part, key_part)) = line.split_once('=') else {
            warnings.push(format!(
                "line {line_no}: expected `action-id = X`, got {raw_line:?}"
            ));
            continue;
        };
        let id = id_part.trim();
        let Some(action) = CONFIGURABLE_ACTIONS.iter().find(|a| a.id == id) else {
            warnings.push(format!(
                "line {line_no}: {id:?} is not a remappable action (see docs/architecture/preferences-matrix.md for the documented list); ignored"
            ));
            continue;
        };
        let key_spec = key_part.trim();
        let mut chars = key_spec.chars();
        let (Some(key), None) = (chars.next(), chars.next()) else {
            warnings.push(format!(
                "line {line_no}: {key_spec:?} is not a single character; ignored"
            ));
            continue;
        };
        overrides.insert(action.id.to_string(), key);
    }

    ParsedConfig { overrides, warnings }
}

/// Merges each [`CONFIGURABLE_ACTIONS`] entry's default key with any
/// accepted override, producing the key -> [`Action`] table
/// [`crate::keymap::resolve_action`] consults. An override for an id
/// outside the registry (defensively handled even though [`parse_config`]
/// never produces one) is simply ignored, mirroring
/// `apps/desktop/src/keybindings.ts::effectiveBindings`'s own "never
/// invents bindings for actions outside the registry" contract.
pub fn effective_bindings(overrides: &HashMap<String, char>) -> HashMap<char, Action> {
    let mut result = HashMap::new();
    for action in CONFIGURABLE_ACTIONS {
        let key = overrides.get(action.id).copied().unwrap_or(action.default_key);
        result.insert(key, action.action);
    }
    result
}

/// Groups every configurable action's *id* by its effective key, keeping
/// only keys shared by two or more actions — mirrors
/// `apps/desktop/src/keybindings.ts::findBindingConflicts`'s own "reported,
/// never silently prevented" convention (a settings/help surface can
/// choose to warn a person about this; [`effective_bindings`] itself still
/// resolves deterministically, last entry in [`CONFIGURABLE_ACTIONS`]'s
/// own order wins, since it inserts into the same key in that order).
pub fn find_conflicts(overrides: &HashMap<String, char>) -> HashMap<char, Vec<&'static str>> {
    let mut by_key: HashMap<char, Vec<&'static str>> = HashMap::new();
    for action in CONFIGURABLE_ACTIONS {
        let key = overrides.get(action.id).copied().unwrap_or(action.default_key);
        by_key.entry(key).or_default().push(action.id);
    }
    by_key.retain(|_, ids| ids.len() > 1);
    by_key
}

/// The default on-disk location for the keybindings-override file: `<OS
/// config dir>/gitsail/tui/keybindings.conf`, mirroring
/// `JsonFilePreferencesStore::default_location`'s own `gitsail/<app>/...`
/// convention (a sibling directory to Desktop's own `gitsail/desktop/`).
/// `None` when the OS config directory cannot be resolved — the caller
/// (`main.rs`) treats that exactly like "no file present", never a fatal
/// error: the TUI must always start with its documented defaults.
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("gitsail").join("tui").join("keybindings.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::InputContext;

    #[test]
    fn the_registry_never_lists_activate_or_dismiss() {
        assert!(
            CONFIGURABLE_ACTIONS
                .iter()
                .all(|a| a.action != Action::Activate && a.action != Action::Dismiss),
            "Activate/Dismiss drive OperationState::confirm/cancel directly \
             and must never be remappable (T-251 criterion 3)"
        );
    }

    #[test]
    fn every_registered_action_is_only_meaningful_in_the_normal_context() {
        // A defensive documentation check: every default key here already
        // has a fixed meaning in `InputContext::Normal` per `keymap.rs`,
        // confirming this registry is a curated subset of that context's
        // own keys, not a parallel invention.
        for action in CONFIGURABLE_ACTIONS {
            use crossterm::event::{KeyCode, KeyEvent};
            let key = KeyEvent::new(KeyCode::Char(action.default_key), crossterm::event::KeyModifiers::NONE);
            assert_eq!(
                crate::keymap::action_for(key, InputContext::Normal),
                Some(action.action),
                "registry entry {:?} disagrees with keymap.rs's own Normal-context default",
                action.id
            );
        }
    }

    #[test]
    fn defaults_resolve_when_nothing_is_overridden() {
        let bindings = effective_bindings(&HashMap::new());
        for action in CONFIGURABLE_ACTIONS {
            assert_eq!(bindings.get(&action.default_key), Some(&action.action));
        }
    }

    #[test]
    fn parsing_a_valid_override_remaps_only_that_action() {
        let parsed = parse_config("quit = Q\n# a comment\n\nrefresh = R\n");
        assert_eq!(parsed.warnings, Vec::<String>::new());
        assert_eq!(parsed.overrides.get("quit"), Some(&'Q'));
        assert_eq!(parsed.overrides.get("refresh"), Some(&'R'));

        let bindings = effective_bindings(&parsed.overrides);
        assert_eq!(bindings.get(&'Q'), Some(&Action::Quit));
        assert_eq!(bindings.get(&'q'), None, "the old default key is no longer bound");
        assert_eq!(bindings.get(&'?'), Some(&Action::ToggleHelp), "an unrelated action keeps its default");
    }

    #[test]
    fn an_unknown_action_id_is_rejected_never_silently_applied() {
        let parsed = parse_config("made-up-action = z");
        assert!(parsed.overrides.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("made-up-action"));
    }

    /// The load-bearing case: an override file cannot name the two actions
    /// that drive confirmation, because they are simply not in the
    /// registry `parse_config` resolves ids against (T-251 criterion 3).
    #[test]
    fn attempting_to_remap_activate_or_dismiss_is_rejected() {
        for id in ["activate", "dismiss", "confirm", "cancel"] {
            let parsed = parse_config(&format!("{id} = x"));
            assert!(
                parsed.overrides.is_empty(),
                "{id:?} must never be accepted as a remappable action"
            );
            assert_eq!(parsed.warnings.len(), 1);
        }
    }

    #[test]
    fn a_malformed_line_is_reported_and_ignored() {
        let parsed = parse_config("not-a-valid-line\nquit = ab\nrefresh = \n");
        assert!(parsed.overrides.is_empty());
        assert_eq!(parsed.warnings.len(), 3);
    }

    #[test]
    fn conflicts_are_detected_but_still_resolve_deterministically() {
        // Shadow `toggle-help`'s default ('?') onto `quit`'s key.
        let mut overrides = HashMap::new();
        overrides.insert("toggle-help".to_string(), 'q');

        let conflicts = find_conflicts(&overrides);
        let ids = conflicts.get(&'q').cloned().unwrap_or_default();
        assert_eq!(ids, vec!["quit", "toggle-help"]);

        // `effective_bindings` still resolves to exactly one action —
        // whichever is later in `CONFIGURABLE_ACTIONS`'s own fixed order —
        // never an ambiguous/undefined lookup.
        let bindings = effective_bindings(&overrides);
        assert_eq!(bindings.get(&'q'), Some(&Action::ToggleHelp));
    }
}
