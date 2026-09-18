//! `gitsail-tui`: the interactive, keyboard-first GitSail interface (SAD
//! §18, §37; ADR-005; US-040..US-043).
//!
//! Wires the real `gitsail-git` adapter into [`gitsail_tui::App`] and runs
//! the render loop: draw the current state, wait for the next
//! [`gitsail_tui::Message`] (terminal input, a tick, or a background
//! [`gitsail_tui::Command`] result), fold it into the state, repeat. Git
//! process execution never happens here — only in `worker::dispatch`'s
//! background threads (US-041 criterion 2).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use gitsail_application::{RepositoryReadPort, RepositoryWritePort};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
use gitsail_tui::{event, keymap, terminal, ui, worker, App, Message};

/// GitSail — interactive terminal interface.
#[derive(Debug, Parser)]
#[command(name = "gitsail-tui", version, about, long_about = None)]
struct Cli {
    /// Path to the repository to open (default: current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Explicit path to the `git` executable (default: `git` on PATH).
    #[arg(long)]
    git_path: Option<PathBuf>,

    /// Disable color and rely on text/markers alone to distinguish state
    /// (US-043 criterion 3). Also enabled automatically when `NO_COLOR` is
    /// set, per that convention.
    #[arg(long)]
    ascii: bool,
}

const TICK_RATE: Duration = Duration::from_millis(250);

fn main() {
    let cli = Cli::parse();
    let low_color = cli.ascii || std::env::var_os("NO_COLOR").is_some();

    let runner_config = GitProcessRunnerConfig {
        executable: cli.git_path.clone(),
        default_cwd: None,
        default_timeout: None,
    };
    let runner = match GitProcessRunner::new(runner_config) {
        Ok(runner) => runner,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    };
    let provider = Arc::new(GitCliProvider::new(runner));
    let read_port: Arc<dyn RepositoryReadPort> = provider.clone();
    let write_port: Arc<dyn RepositoryWritePort> = provider;

    let mut tui = match terminal::init() {
        Ok(tui) => tui,
        Err(err) => {
            eprintln!("error: failed to initialize terminal: {err}");
            std::process::exit(1);
        }
    };

    let result = run(&mut tui, cli.repo, read_port, write_port, low_color);

    // Always restore the terminal on the way out, whether `run` returned
    // `Ok` or `Err` (US-043 criterion 1). A panic is covered separately by
    // the hook `terminal::init` installs.
    let restore_result = terminal::restore();

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
    if let Err(err) = restore_result {
        eprintln!("error: failed to restore terminal: {err}");
        std::process::exit(1);
    }
}

fn run(
    tui: &mut terminal::Tui,
    repo_path: PathBuf,
    read_port: Arc<dyn RepositoryReadPort>,
    write_port: Arc<dyn RepositoryWritePort>,
    low_color: bool,
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
                match keymap::action_for(key, app.input_context()) {
                    Some(action) => app.update(action),
                    None => Vec::new(),
                }
            }
            Message::Term(crossterm::event::Event::Resize(width, height)) => {
                app.handle_resize(width, height);
                Vec::new()
            }
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
        };
        worker::dispatch(commands, &read_port, &write_port, &tx);

        if app.should_quit() {
            break;
        }
    }

    Ok(())
}
