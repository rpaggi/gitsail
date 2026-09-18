//! Integration tests for T-238/US-086 (cherry-pick), T-239/US-087 (revert),
//! and T-240/US-088 (reset) against a real, temporary Git repository via
//! `GitCliProvider` (never a mock) — mirroring `tests/rebase.rs`'s own
//! fixture and `run_mutation` drive-loop conventions.

mod support;

use gitsail_application::{
    CherryPickResult, GetCommitHistory, GetRepositoryStatus, ListBranches, RevertResult,
};
use gitsail_tui::{Action, App, Command, OperationKind, OperationState};
use support::{git, read_port, write_port, TempDir};

fn init_repo(dir: &std::path::Path) {
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["config", "user.email", "test@example.com"]);
}

fn current_head(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Opens `dir`, drives `app` to the loaded phase (status, branches and the
/// first commit-graph page all refreshed against the real adapter) —
/// mirrors `tests/history_explorer.rs::open_and_load`, extended with the
/// branches load `tests/rebase.rs::open_and_load` also drives.
fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir)
        .unwrap();
    let commands = app.on_repository_opened(Ok(repo.clone()));

    let ticket = commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port.clone()).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
    app.on_in_progress_operation_loaded(generation, port.detect_in_progress_operation(&repo));

    for command in commands {
        if let Command::LoadCommitGraph(request_id, repo, query) = command {
            let result = GetCommitHistory::new(port.clone()).execute(&repo, &query);
            app.on_commit_graph_page_loaded(request_id, result);
        }
    }
}

/// Submits `text` as a commit search from the Graph panel and runs the
/// resulting `LoadCommitGraph` command against the real adapter, mirroring
/// `tests/history_explorer.rs::submit_search`.
fn submit_search(app: &mut App, dir: &std::path::Path, text: &str) {
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir)
        .unwrap();

    app.update(Action::StartSearch);
    while app.commit_search().is_some_and(|s| !s.is_empty()) {
        app.update(Action::CommitSearchBackspace);
    }
    for c in text.chars() {
        app.update(Action::CommitSearchInput(c));
    }
    let commands = app.update(Action::CommitSearchSubmit);
    let (request_id, query) = match commands.as_slice() {
        [Command::LoadCommitGraph(id, _, q)] => (*id, q.clone()),
        other => panic!("expected exactly one LoadCommitGraph command, got {other:?}"),
    };
    let result = GetCommitHistory::new(port).execute(&repo, &query);
    app.on_commit_graph_page_loaded(request_id, result);
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — mirrors `tests/rebase.rs::run_mutation`,
/// extended with this epic's own new `Command` variants.
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::CherryPick(repo, commit, merge_parent) => {
                let result = gitsail_application::CherryPick::new(write.clone()).execute(
                    &repo,
                    &commit,
                    merge_parent,
                );
                app.on_cherry_pick_finished(result)
            }
            Command::Revert(repo, commit, merge_parent) => {
                let result = gitsail_application::Revert::new(write.clone()).execute(
                    &repo,
                    &commit,
                    merge_parent,
                );
                app.on_revert_finished(result)
            }
            Command::Reset(repo, target, mode, expected_head) => {
                let result = gitsail_application::Reset::new(write.clone()).execute(
                    &repo,
                    &target,
                    mode,
                    &expected_head,
                );
                app.on_operation_finished(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(read.clone()).execute(&repo);
                app.on_status_refreshed(ticket, result);
                Vec::new()
            }
            Command::LoadBranches(generation, repo) => {
                let result = ListBranches::new(read.clone()).execute(&repo);
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

fn focus_graph(app: &mut App) {
    while app.focus() != gitsail_tui::Panel::Graph {
        app.update(Action::FocusNext);
    }
}

/// `main` with two commits — a base and one adding `feature.txt` from its
/// own now-merged `feature` branch — plus that `feature` branch's own
/// exclusive commit still reachable by name (not by walking `HEAD`), so a
/// Graph search for `branch:feature` can select it for cherry-pick/revert
/// even though `HEAD` (currently `main`) never walks through it directly.
fn setup_feature_branch_with_one_commit(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("feature.txt"), "feature content\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
}

#[test]
fn cherry_picking_the_highlighted_graph_commit_applies_it_via_the_tui_flow() {
    let dir = TempDir::new("cherry-pick-tui");
    setup_feature_branch_with_one_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    focus_graph(&mut app);
    submit_search(&mut app, dir.path(), "branch:feature");
    let highlighted = app
        .selected_graph_commit()
        .expect("feature's own commit must be highlighted")
        .clone();
    assert_eq!(highlighted.subject, "feature change");

    app.update(Action::RequestCherryPick);
    match app.operation() {
        OperationState::Confirming(kind @ OperationKind::CherryPick { .. }) => {
            assert!(kind.target_label().contains(highlighted.hash.as_str()));
        }
        other => panic!("expected Confirming(CherryPick), got {other:?}"),
    }

    let commands = app.update(Action::Activate); // confirms -> dispatches
    run_mutation(&mut app, commands);

    match app.last_cherry_pick_result() {
        Some(CherryPickResult::Applied { .. }) => {}
        other => panic!("expected Applied, got {other:?}"),
    }
    assert!(
        dir.path().join("feature.txt").is_file(),
        "the cherry-picked commit's file must now exist on main"
    );
}

#[test]
fn reverting_the_highlighted_graph_commit_undoes_it_via_the_tui_flow() {
    let dir = TempDir::new("revert-tui");
    init_repo(dir.path());
    std::fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "base"]);
    std::fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "add line2"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    focus_graph(&mut app);
    let highlighted = app
        .selected_graph_commit()
        .expect("the newest commit must be highlighted by default")
        .clone();
    assert_eq!(highlighted.subject, "add line2");

    app.update(Action::RequestRevert);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Revert { .. })
    ));

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    match app.last_revert_result() {
        Some(RevertResult::Applied { .. }) => {}
        other => panic!("expected Applied, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "line1\n",
        "the revert must undo exactly the reverted commit's change"
    );
}

#[test]
fn resetting_hard_via_the_mode_chooser_moves_head_index_and_working_tree() {
    let dir = TempDir::new("reset-hard-tui");
    init_repo(dir.path());
    std::fs::write(dir.path().join("f.txt"), "a\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "c1"]);
    let c1 = current_head(dir.path());
    std::fs::write(dir.path().join("f.txt"), "a\nb\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "c2"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    // The Graph panel's default (newest-first) order puts `c2` at the
    // cursor; move down once to highlight `c1`, the reset target.
    app.update(Action::MoveDown);
    let target = app
        .selected_graph_commit()
        .expect("c1 must be reachable one row down")
        .clone();
    assert_eq!(target.subject, "c1");

    app.update(Action::RequestReset);
    assert!(app.reset_mode_open());
    assert_eq!(app.reset_mode_cursor(), 0, "the chooser defaults to Soft");

    // Cycle down to Hard (Soft -> Mixed -> Hard).
    app.update(Action::MoveDown);
    app.update(Action::MoveDown);
    assert_eq!(app.reset_mode_cursor(), 2);

    let confirm_commands = app.update(Action::Activate); // chooser -> Confirming
    assert!(
        confirm_commands.is_empty(),
        "choosing a mode only starts confirmation"
    );
    match app.operation() {
        OperationState::Confirming(OperationKind::Reset {
            mode, target: t, ..
        }) => {
            assert_eq!(*mode, gitsail_application::ResetMode::Hard);
            assert_eq!(t, target.hash.as_str());
        }
        other => panic!("expected Confirming(Reset), got {other:?}"),
    }

    let commands = app.update(Action::Activate); // confirms -> dispatches
    run_mutation(&mut app, commands);

    assert_eq!(
        current_head(dir.path()),
        c1,
        "HEAD must now point at the reset target"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "a\n",
        "a hard reset makes the working tree identical to the target"
    );
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        String::from_utf8(status_output.stdout)
            .unwrap()
            .trim()
            .is_empty(),
        "a hard reset leaves nothing staged or unstaged"
    );
}
