//! Integration tests for US-047 ("Preparar e criar commits na TUI")
//! against a real, temporary Git repository via `GitCliProvider` (never a
//! mock).

mod support;

use gitsail_application::{GetRepositoryStatus, OpenRepository};
use gitsail_tui::{ui, Action, App, Command};
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

fn focus_details(app: &mut App) {
    app.update(Action::FocusNext); // Sidebar -> Graph
    app.update(Action::FocusNext); // Graph -> Details
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — including chasing the `RefreshStatus`/
/// `LoadBranches` follow-up commands a successful mutation returns, so
/// `app`'s status/branches genuinely reflect the mutation afterwards.
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::StageFiles(repo, paths) => {
                let result =
                    gitsail_application::StageFiles::new(write.clone()).execute(&repo, &paths);
                app.on_operation_finished(result)
            }
            Command::UnstageFiles(repo, paths) => {
                let result =
                    gitsail_application::UnstageFiles::new(write.clone()).execute(&repo, &paths);
                app.on_operation_finished(result)
            }
            Command::CreateCommit(repo, message) => {
                let result =
                    gitsail_application::CreateCommit::new(write.clone()).execute(&repo, &message);
                app.on_commit_created(result)
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
fn everyday_scenario_stages_a_file_and_commits_it_successfully() {
    let dir = TempDir::new("commit-everyday");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    // Stage the one worktree entry (US-047 criterion 1: shared use case).
    let commands = app.update(Action::ToggleStage);
    assert!(matches!(commands.as_slice(), [Command::StageFiles(_, _)]));
    run_mutation(&mut app, commands);
    assert!(
        app.operation().is_idle(),
        "a Safe operation's success must not linger as a state needing dismissal"
    );
    assert!(porcelain_status(dir.path()).contains("M  README.md"));

    // Compose and commit (criterion 2: composer shows the staged scope).
    app.update(Action::StartCommit);
    for c in "fix readme".chars() {
        app.update(Action::CommitMessageInput(c));
    }
    let composer_text = render(&app);
    assert!(
        composer_text.contains("README.md"),
        "staged scope must be visible:\n{composer_text}"
    );
    assert!(
        composer_text.contains("fix readme"),
        "typed message must be visible:\n{composer_text}"
    );

    app.update(Action::Activate); // idle -> confirming
    let commands = app.update(Action::Activate); // confirming -> dispatch
    assert!(matches!(commands.as_slice(), [Command::CreateCommit(_, _)]));
    run_mutation(&mut app, commands);

    assert_eq!(last_commit_subject(dir.path()), "fix readme");
    assert!(app.commit_message().is_none());
    assert_eq!(
        porcelain_status(dir.path()),
        "",
        "the worktree must be clean after the commit"
    );
}

#[cfg(unix)]
#[test]
fn a_failing_pre_commit_hook_preserves_the_message_and_the_staged_index() {
    use std::os::unix::fs::PermissionsExt;
    // Scoped here for the same reason as `PermissionsExt`: this is the only
    // test that inspects `OperationState`, and at module scope it is a dead
    // import on Windows, where this `cfg(unix)` test does not compile.
    use gitsail_tui::OperationState;

    let dir = TempDir::new("commit-hook-failure");
    init_repo_with_initial_commit(dir.path());

    let hooks_dir = dir.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    std::fs::write(&hook_path, "#!/bin/sh\necho blocked by hook >&2\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&hook_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&hook_path, perms).unwrap();

    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    let commands = app.update(Action::ToggleStage);
    run_mutation(&mut app, commands);
    let before_commit_log = last_commit_subject(dir.path());

    app.update(Action::StartCommit);
    for c in "blocked".chars() {
        app.update(Action::CommitMessageInput(c));
    }
    app.update(Action::Activate);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert_eq!(
        app.commit_message(),
        Some("blocked"),
        "a failed commit must preserve the typed message"
    );
    assert!(matches!(app.operation(), OperationState::Failed(_, _)));
    assert!(
        porcelain_status(dir.path()).contains("M  README.md"),
        "the staged index must survive a failed commit:\n{}",
        porcelain_status(dir.path())
    );
    assert_eq!(
        last_commit_subject(dir.path()),
        before_commit_log,
        "no new commit must have been created"
    );

    // The composer shows the error's user-safe `message()`, never the raw
    // hook stderr — that stays in `diagnostic()` by design (SAD §19, §28),
    // checked directly here rather than expecting it to be rendered.
    let composer_text = render(&app);
    assert!(
        composer_text.contains("non-zero status") || composer_text.contains("dismisses"),
        "the failure must be visible in the composer:\n{composer_text}"
    );
    match app.operation() {
        OperationState::Failed(_, err) => {
            assert!(
                err.diagnostic()
                    .map(|d| d.to_string().contains("blocked by hook"))
                    .unwrap_or(false),
                "the hook's raw stderr must still be attached as diagnostic detail"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn unstaging_a_staged_entry_returns_it_to_the_worktree() {
    let dir = TempDir::new("commit-unstage");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();
    git(dir.path(), &["add", "README.md"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    let commands = app.update(Action::ToggleStage);
    assert!(matches!(commands.as_slice(), [Command::UnstageFiles(_, _)]));
    run_mutation(&mut app, commands);

    assert!(porcelain_status(dir.path()).contains(" M README.md"));
}
