//! Integration tests for T-242/US-090 ("Disponibilizar amend na TUI")
//! against a real, temporary Git repository via `GitCliProvider` (never a
//! mock). US-090 criterion 1: the TUI reuses `gitsail_application`'s
//! existing `PreviewAmend`/`AmendCommit` use cases unchanged — the same
//! ones `apps/desktop`'s amend flow (T-192) already exercises — so these
//! tests exercise the real Core capability through the TUI's own
//! `Action`/`Command`/`Message` plumbing, never a parallel Git
//! implementation.

mod support;

use gitsail_application::{GetCommitHistory, GetRepositoryStatus, OpenRepository};
use gitsail_tui::{ui, Action, App, Command, OperationState};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, write_port, TempDir};

fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let port = read_port();
    let repo = OpenRepository::new(port.clone()).execute(dir).unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));
    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — mirrors `commit_composer.rs`'s own
/// `run_mutation`, extended with the amend-specific and commit-graph
/// commands a successful amend also issues (US-090 criterion 3: "sucesso
/// refresca o commit graph e o status").
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::PreviewAmend(repo) => {
                let result = gitsail_application::PreviewAmend::new(read.clone())
                    .execute(&repo, &gitsail_domain::CancellationToken::new());
                app.on_amend_previewed(result);
                Vec::new()
            }
            Command::AmendCommit(repo, message, expected_head) => {
                let result = gitsail_application::AmendCommit::new(write.clone()).execute(
                    &repo,
                    &message,
                    &expected_head,
                );
                app.on_amend_finished(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(read.clone()).execute(&repo);
                app.on_status_refreshed(ticket, result);
                Vec::new()
            }
            Command::LoadBranches(generation, repo) => {
                let result = gitsail_application::ListBranches::new(read.clone()).execute(&repo);
                app.on_branches_loaded(generation, result);
                Vec::new()
            }
            Command::LoadTags(generation, repo) => {
                app.on_tags_loaded(generation, read.list_tags(&repo));
                Vec::new()
            }
            Command::LoadRemotes(generation, repo) => {
                app.on_remotes_loaded(generation, read.list_remotes(&repo));
                Vec::new()
            }
            Command::LoadStashEntries(generation, repo) => {
                app.on_stash_entries_loaded(generation, read.list_stash_entries(&repo));
                Vec::new()
            }
            Command::LoadReflog(generation, repo) => {
                app.on_reflog_loaded(generation, read.reflog(&repo));
                Vec::new()
            }
            Command::LoadInProgressOperation(generation, repo) => {
                app.on_in_progress_operation_loaded(
                    generation,
                    read.detect_in_progress_operation(&repo),
                );
                Vec::new()
            }
            Command::LoadCommitGraph(request_id, repo, query) => {
                let result = GetCommitHistory::new(read.clone()).execute(&repo, &query);
                app.on_commit_graph_page_loaded(request_id, result);
                Vec::new()
            }
            other => panic!("unexpected command: {other:?}"),
        };
        queue.extend(follow_up);
    }
}

fn last_commit_subject(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn current_head(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn porcelain_status(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    buffer_text(&terminal)
}

#[test]
fn amending_head_replaces_it_with_the_edited_message_and_folds_in_staged_changes() {
    let dir = TempDir::new("amend-everyday");
    init_repo_with_initial_commit(dir.path());
    let original_head = current_head(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();
    git(dir.path(), &["add", "README.md"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    // US-090 criterion 1: opening the composer previews HEAD's identity and
    // the staged diff read-only, pre-filling the message from HEAD's own
    // subject (mirrors `apps/desktop`'s `loadPreview`).
    let commands = app.update(Action::StartAmend);
    assert!(matches!(commands.as_slice(), [Command::PreviewAmend(_)]));
    run_mutation(&mut app, commands);
    assert_eq!(app.amend_message(), Some("initial commit"));
    let preview_text = render(&app);
    assert!(
        preview_text.contains("README.md") || preview_text.contains("1 staged file"),
        "the composer must show the staged scope:\n{preview_text}"
    );

    // Replace the message.
    for _ in 0.."initial commit".chars().count() {
        app.update(Action::AmendMessageBackspace);
    }
    for c in "amended message".chars() {
        app.update(Action::AmendMessageInput(c));
    }

    // US-090 criterion 2: confirming shows the exact replaced commit and the
    // Destructive publication-risk warning before anything runs.
    app.update(Action::Activate); // idle -> confirming
    let confirm_text = render(&app);
    assert!(
        confirm_text.contains("pushed or shared"),
        "the destructive publication-risk warning must be shown:\n{confirm_text}"
    );

    let commands = app.update(Action::Activate); // confirming -> dispatch
    assert!(matches!(
        commands.as_slice(),
        [Command::AmendCommit(_, _, _)]
    ));
    run_mutation(&mut app, commands);

    assert!(matches!(app.operation(), OperationState::Succeeded(_)));
    assert!(!app.amend_open(), "success must close the composer");
    assert_eq!(last_commit_subject(dir.path()), "amended message");
    assert_ne!(
        current_head(dir.path()),
        original_head,
        "amend must replace HEAD with a new commit object"
    );
    assert_eq!(
        porcelain_status(dir.path()),
        "",
        "the staged change must have been folded into the amended commit"
    );
}

/// US-090 criterion 1 / DoD: mirrors the Desktop's own stale-`HEAD` race
/// test. `PreviewAmend` captures `HEAD` at preview time; if another process
/// (here, a plain `git commit` run directly, standing in for another
/// terminal/editor) advances `HEAD` before the amend is confirmed, the real
/// `RepositoryWritePort::amend_commit` revalidation must refuse rather than
/// rewrite whatever `HEAD` happens to be now — and US-090 criterion 3
/// requires the typed message to survive that refusal.
#[test]
fn a_head_that_moved_since_the_preview_is_refused_and_the_message_is_preserved() {
    let dir = TempDir::new("amend-stale-head");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    let commands = app.update(Action::StartAmend);
    run_mutation(&mut app, commands);
    assert_eq!(app.amend_message(), Some("initial commit"));

    for c in " — edited".chars() {
        app.update(Action::AmendMessageInput(c));
    }

    // Another process commits in the meantime — HEAD moves out from under
    // the already-loaded preview.
    git(
        dir.path(),
        &["commit", "--allow-empty", "-q", "-m", "concurrent commit"],
    );
    let head_after_concurrent_commit = current_head(dir.path());

    app.update(Action::Activate); // idle -> confirming
    let commands = app.update(Action::Activate); // confirming -> dispatch
    assert!(matches!(
        commands.as_slice(),
        [Command::AmendCommit(_, _, _)]
    ));
    run_mutation(&mut app, commands);

    assert!(
        matches!(app.operation(), OperationState::Failed(_, _)),
        "a stale HEAD must never be silently amended"
    );
    assert_eq!(
        app.amend_message(),
        Some("initial commit — edited"),
        "the typed message must survive a rejected amend"
    );
    assert!(
        app.amend_open(),
        "the composer must still be showing the preserved message"
    );
    assert_eq!(
        current_head(dir.path()),
        head_after_concurrent_commit,
        "a refused amend must never move HEAD"
    );
    assert_eq!(last_commit_subject(dir.path()), "concurrent commit");
}
