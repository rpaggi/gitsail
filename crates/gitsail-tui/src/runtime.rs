//! The TUI's reusable entry point (extracted from `src/main.rs` so
//! `gitsail-cli`'s own binary can drop into the same interactive interface
//! when invoked with no subcommand, without duplicating any of this logic
//! or depending on a second binary).
//!
//! [`run_interactive`] is the single thing both `gitsail-tui`'s own
//! `main.rs` and `gitsail-cli`'s `main.rs` call — argument parsing (each
//! binary's own `clap::Parser`) stays in each binary, everything after
//! "here is a resolved repo path, git executable override, color
//! preference, and keybindings file" is common and lives here exactly
//! once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use gitsail_application::{RepositoryReadPort, RepositoryWritePort};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

use crate::{event, keybindings, keymap, terminal, ui, worker, Action, App, Message};

const TICK_RATE: Duration = Duration::from_millis(250);

/// Loads the keybindings-override file at `path` (or the platform default
/// when `path` is `None`), reporting any rejected line to stderr and
/// always returning a usable bindings table — a missing file, an
/// unreadable file, or one with only invalid lines all fall back to
/// [`keybindings::CONFIGURABLE_ACTIONS`]'s own defaults, mirroring
/// `gitsail_application::preferences`'s "invalid input degrades to safe
/// defaults, never a crash" convention.
fn load_bindings(path: Option<&Path>) -> HashMap<char, Action> {
    let resolved_path = path
        .map(Path::to_path_buf)
        .or_else(keybindings::default_config_path);
    let contents = resolved_path.and_then(|p| std::fs::read_to_string(p).ok());
    let overrides = match contents {
        Some(contents) => {
            let parsed = keybindings::parse_config(&contents);
            for warning in &parsed.warnings {
                eprintln!("warning: keybindings config: {warning}");
            }
            parsed.overrides
        }
        None => HashMap::new(),
    };
    keybindings::effective_bindings(&overrides)
}

/// Runs the interactive TUI to completion and returns the process exit
/// code for it. Callers (either binary's `main`) should return/propagate
/// this directly.
///
/// `low_color` should already fold in each caller's own `NO_COLOR`/`--ascii`
/// precedence (both binaries treat `NO_COLOR` being set as equivalent to
/// `--ascii`, per US-043 criterion 3) — this function does not re-derive it,
/// so the two binaries cannot silently drift on that policy.
pub fn run_interactive(
    repo: PathBuf,
    git_path: Option<PathBuf>,
    low_color: bool,
    keybindings_path: Option<PathBuf>,
) -> ExitCode {
    let bindings = load_bindings(keybindings_path.as_deref());

    let runner_config = GitProcessRunnerConfig {
        executable: git_path,
        default_cwd: None,
        default_timeout: None,
    };
    let runner = match GitProcessRunner::new(runner_config) {
        Ok(runner) => runner,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let provider = Arc::new(GitCliProvider::new(runner));
    let read_port: Arc<dyn RepositoryReadPort> = provider.clone();
    let write_port: Arc<dyn RepositoryWritePort> = provider;

    let mut tui = match terminal::init() {
        Ok(tui) => tui,
        Err(err) => {
            eprintln!("error: failed to initialize terminal: {err}");
            return ExitCode::FAILURE;
        }
    };

    let result = run_loop(&mut tui, repo, read_port, write_port, low_color, &bindings);

    // Always restore the terminal on the way out, whether `run_loop`
    // returned `Ok` or `Err` (US-043 criterion 1). A panic is covered
    // separately by the hook `terminal::init` installs.
    let restore_result = terminal::restore();

    if let Err(err) = result {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }
    if let Err(err) = restore_result {
        eprintln!("error: failed to restore terminal: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_loop(
    tui: &mut terminal::Tui,
    repo_path: PathBuf,
    read_port: Arc<dyn RepositoryReadPort>,
    write_port: Arc<dyn RepositoryWritePort>,
    low_color: bool,
    bindings: &HashMap<char, Action>,
) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<Message>();
    event::spawn(tx.clone(), TICK_RATE);

    let (mut app, initial_commands) = App::new(repo_path, Arc::clone(&read_port), low_color);
    worker::dispatch(initial_commands, &read_port, &write_port, &tx);

    loop {
        tui.draw(|frame| ui::render(frame, &app))?;

        let message = match rx.recv() {
            Ok(message) => message,
            // Every sender (the input thread, and one thread per
            // in-flight background command) has exited; nothing left to
            // wait for.
            Err(_) => break,
        };

        let commands = match message {
            Message::Term(crossterm::event::Event::Key(key)) => {
                match keymap::resolve_action(key, app.input_context(), bindings) {
                    Some(action) => app.update(action),
                    None => Vec::new(),
                }
            }
            Message::Term(crossterm::event::Event::Resize(width, height)) => {
                app.handle_resize(width, height);
                Vec::new()
            }
            // Regaining OS-level focus re-detects the in-progress operation
            // and the rest of the refresh set (T-234/US-082 criterion 2),
            // mirroring `apps/desktop`'s own window-focus refresh. Enabled
            // by `terminal::init`'s `EnableFocusChange`.
            Message::Term(crossterm::event::Event::FocusGained) => app.on_focus_gained(),
            Message::Term(_) => Vec::new(),
            Message::Tick => Vec::new(),
            Message::RepositoryOpened(result) => app.on_repository_opened(result),
            Message::StatusRefreshed(ticket, result) => {
                app.on_status_refreshed(ticket, result);
                Vec::new()
            }
            Message::BranchesLoaded(generation, result) => {
                app.on_branches_loaded(generation, result);
                Vec::new()
            }
            Message::DiffLoaded(request_id, result) => {
                app.on_diff_loaded(request_id, result);
                Vec::new()
            }
            Message::BlameLoaded(request_id, result) => {
                app.on_blame_loaded(request_id, result);
                Vec::new()
            }
            Message::CommitGraphPageLoaded(request_id, result) => {
                app.on_commit_graph_page_loaded(request_id, result);
                Vec::new()
            }
            Message::OperationFinished(result) => app.on_operation_finished(result),
            Message::CommitCreated(result) => app.on_commit_created(result),
            Message::TagsLoaded(generation, result) => {
                app.on_tags_loaded(generation, result);
                Vec::new()
            }
            Message::RemotesLoaded(generation, result) => {
                app.on_remotes_loaded(generation, result);
                Vec::new()
            }
            Message::StashEntriesLoaded(generation, result) => {
                app.on_stash_entries_loaded(generation, result);
                Vec::new()
            }
            Message::PullFinished(result) => app.on_pull_finished(result),
            Message::PatchPreviewed(result, patch_text) => {
                app.on_patch_previewed(result, patch_text);
                Vec::new()
            }
            Message::PatchApplied(result) => app.on_patch_applied(result),
            Message::InProgressOperationLoaded(generation, result) => {
                app.on_in_progress_operation_loaded(generation, result);
                Vec::new()
            }
            Message::MergeFinished(result) => app.on_merge_finished(result),
            Message::ConflictSidesLoaded(path, result) => {
                app.on_conflict_sides_loaded(path, result);
                Vec::new()
            }
            Message::ConflictResolutionFinished(result) => {
                app.on_conflict_resolution_finished(result)
            }
            Message::OperationResolutionFinished(result) => {
                app.on_operation_resolution_finished(result)
            }
            Message::RebaseFinished(result) => app.on_rebase_finished(result),
            Message::RebasePlanLoaded(result) => {
                app.on_rebase_plan_loaded(result);
                Vec::new()
            }
            Message::CherryPickFinished(result) => app.on_cherry_pick_finished(result),
            Message::RevertFinished(result) => app.on_revert_finished(result),
            Message::ReflogLoaded(generation, result) => {
                app.on_reflog_loaded(generation, result);
                Vec::new()
            }
            Message::ReflogCommitLoaded(hash, result) => {
                app.on_reflog_commit_loaded(hash, result);
                Vec::new()
            }
            Message::AmendPreviewed(result) => {
                app.on_amend_previewed(result);
                Vec::new()
            }
            Message::AmendCommitFinished(result) => app.on_amend_finished(result),
            Message::UrlOpened(result) => {
                app.on_url_opened(result);
                Vec::new()
            }
        };
        worker::dispatch(commands, &read_port, &write_port, &tx);

        if app.should_quit() {
            break;
        }
    }

    Ok(())
}
