//! GitSail TUI: a Ratatui-based, keyboard-first interface over the shared
//! application core (SAD §18; ADR-005; EPIC-09).
//!
//! The presentation layer never executes or parses Git output itself
//! (US-040 criterion 3): it talks to `gitsail-application` use cases and
//! [`gitsail_application::RepositorySession`] through a
//! [`gitsail_application::RepositoryReadPort`], the same contract the CLI
//! uses, wired here to the real `gitsail-git` adapter.
//!
//! The module split follows the unidirectional event/update/render model
//! from SAD §18 (`Terminal Events -> Action -> Update -> App State ->
//! Render`):
//!
//! - [`event`] turns terminal input into a [`message::Message`] on a
//!   background thread.
//! - [`keymap`] turns one key press into an [`action::Action`], depending
//!   on the current [`keymap::InputContext`] (US-042 criterion 1).
//!   [`keybindings`] documents and resolves the small, fixed set of
//!   commands a person may rebind (T-251/US-109 criterion 2), consulted
//!   only by [`keymap::resolve_action`] and only in the `Normal` context —
//!   see that function's own doc comment for why that boundary is the
//!   safety property T-251 criterion 3 requires.
//! - [`app::App::update`] applies an [`action::Action`] to state and
//!   returns any [`worker::Command`]s it needs run in the background
//!   (US-041 criterion 2: Git process execution never runs here).
//! - [`worker`] executes those `Command`s on background threads and
//!   reports back as a [`message::Message`], tagged so a stale result is
//!   discarded rather than overwriting newer state (US-041 criterion 3).
//! - [`ui`] renders the current [`app::App`] state; it never mutates it.

#![forbid(unsafe_code)]

pub mod action;
pub mod app;
pub mod browser;
pub mod clipboard;
pub mod commit_search;
pub mod event;
pub mod graph_view;
pub mod keybindings;
pub mod keymap;
pub mod message;
pub mod operation;
pub mod runtime;
pub mod sanitize;
pub mod status_view;
pub mod terminal;
pub mod ui;
pub mod worker;

pub use action::Action;
pub use app::{App, DiffViewMode, Panel, PatchExportOutcome, ReferenceView, ViewPhase};
pub use clipboard::{ClipboardPort, FakeClipboard, SystemClipboard};
pub use graph_view::GraphLine;
pub use keymap::InputContext;
pub use message::Message;
pub use operation::{OperationKind, OperationRisk, OperationState};
pub use runtime::run_interactive;
pub use status_view::{DiffScope, StatusEntry};
pub use worker::Command;
