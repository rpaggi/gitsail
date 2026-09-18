//! Integration tests for T-235/US-083 (rebase, skip) against real, temporary
//! Git repositories via `GitCliProvider` (never a mock) — mirroring
//! `tests/merge_conflicts.rs`'s own fixture and `run_mutation` drive-loop
//! conventions.

mod support;

use gitsail_application::{GetRepositoryStatus, ListBranches, RebaseResult};
use gitsail_domain::InProgressOperation;
use gitsail_tui::{Action, App, Command, OperationKind, OperationState};
use support::{git, read_port, write_port, TempDir};

fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir)
        .unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));
    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port.clone()).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port.clone()).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
    app.on_in_progress_operation_loaded(generation, port.detect_in_progress_operation(&repo));
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — mirrors `tests/merge_conflicts.rs::run_mutation`,
/// extended with this epic's own new [`Command`] variants.
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::Merge(repo, target) => {
                let result = gitsail_application::Merge::new(write.clone()).execute(&repo, &target);
                app.on_merge_finished(result)
            }
            Command::Rebase(repo, onto) => {
                let result = gitsail_application::Rebase::new(write.clone()).execute(&repo, &onto);
                app.on_rebase_finished(result)
            }
            Command::SkipOperation(repo) => {
                let result = gitsail_application::SkipOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
            }
            Command::ContinueOperation(repo) => {
                let result = gitsail_application::ContinueOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
            }
            Command::AbortOperation(repo) => {
                let result = gitsail_application::AbortOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
            }
            Command::MarkConflictResolved(repo, path) => {
                let result =
                    gitsail_application::MarkConflictResolved::new(write.clone()).execute(&repo, &path);
                app.on_conflict_resolution_finished(result)
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

fn setup_non_conflicting_divergence(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("feature.txt"), "feature\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "main advances"]);

    git(dir, &["checkout", "-q", "feature"]);
}

fn setup_conflicting_divergence(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("f.txt"), "line1\nline2\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("f.txt"), "line1\nCHANGED-feature\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("f.txt"), "line1\nCHANGED-main\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "main change"]);

    git(dir, &["checkout", "-q", "feature"]);
}

fn select_branch(app: &mut App, name: &str) {
    for _ in 0..app.filtered_branches().len() {
        if app
            .filtered_branches()
            .get(app.sidebar_cursor())
            .map(|b| b.name.as_str() == name)
            .unwrap_or(false)
        {
            return;
        }
        app.update(Action::MoveDown);
    }
    panic!("branch '{name}' was never found under the sidebar cursor");
}

#[test]
fn rebasing_onto_a_reference_reapplies_commits_via_the_tui_flow() {
    let dir = TempDir::new("rebase-clean");
    setup_non_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    app.update(Action::RequestRebase);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Rebase { onto }) if onto == "main"
    ));

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    match app.last_rebase_result() {
        Some(RebaseResult::Completed { new_head }) => {
            assert_eq!(new_head.as_str(), current_head(dir.path()));
        }
        other => panic!("expected Some(Completed), got {other:?}"),
    }
    assert!(app.in_progress_operation().is_none());
    assert!(dir.path().join("b.txt").exists());
    assert!(dir.path().join("feature.txt").exists());
}

#[test]
fn rebase_conflict_opens_a_conflicts_overlay_that_resolves_and_continues() {
    let dir = TempDir::new("rebase-conflict-continue");
    setup_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    app.update(Action::RequestRebase);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    match app.last_rebase_result() {
        Some(RebaseResult::Conflict { files }) => assert_eq!(files.len(), 1),
        other => panic!("expected Some(Conflict), got {other:?}"),
    }
    assert!(matches!(
        app.in_progress_operation(),
        InProgressOperation::Rebase(_)
    ));

    app.update(Action::ToggleConflictsPanel);
    assert!(app.conflicts_open());

    std::fs::write(dir.path().join("f.txt"), "line1\nRESOLVED\nline3\n").unwrap();
    let commands = app.update(Action::MarkConflictResolved);
    run_mutation(&mut app, commands);

    app.update(Action::RequestContinueOperation);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(
        app.in_progress_operation().is_none(),
        "T-235 criterion 3: the real resulting state must be reinspected, not presumed"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "line1\nRESOLVED\nline3\n"
    );
}

#[test]
fn rebase_conflict_recovers_via_skip_from_the_conflicts_overlay() {
    let dir = TempDir::new("rebase-conflict-skip");
    setup_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    app.update(Action::RequestRebase);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(matches!(app.last_rebase_result(), Some(RebaseResult::Conflict { .. })));

    app.update(Action::ToggleConflictsPanel);
    app.update(Action::RequestSkipOperation);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::SkipOperation)
    ));
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(app.in_progress_operation().is_none());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "line1\nCHANGED-main\nline3\n"
    );
}

/// T-235/US-083 criterion 3: skip is never offered for an operation that
/// does not support it (a merge has no further step to skip past) — the
/// request is a no-op rather than reaching the write port.
#[test]
fn request_skip_operation_is_a_no_op_when_a_merge_is_pending() {
    let dir = TempDir::new("skip-no-op-for-merge");
    setup_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(matches!(
        app.in_progress_operation(),
        InProgressOperation::Merge(_)
    ));

    app.update(Action::ToggleConflictsPanel);
    app.update(Action::RequestSkipOperation);

    assert!(
        !matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::SkipOperation)
        ),
        "skip must never be offered for a merge"
    );

    app.update(Action::RequestAbortOperation);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(app.in_progress_operation().is_none());
}
