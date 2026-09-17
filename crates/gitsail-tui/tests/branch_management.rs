//! Integration tests for US-048 ("Administrar branches na TUI") against a
//! real, temporary Git repository via `GitCliProvider` (never a mock).

mod support;

use gitsail_application::{GetRepositoryStatus, ListBranches, OpenRepository};
use gitsail_tui::{Action, App, Command, OperationKind, OperationState};
use support::{git, init_repo_with_initial_commit, read_port, write_port, TempDir};

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
    let status = GetRepositoryStatus::new(port.clone()).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — including chasing the `RefreshStatus`/
/// `LoadBranches` follow-up commands a successful mutation returns, so
/// `app`'s branch list genuinely reflects the mutation afterwards.
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::SwitchBranch(repo, target) => {
                let result = gitsail_application::SwitchBranch::new(write.clone()).execute(&repo, &target);
                app.on_operation_finished(result)
            }
            Command::CreateBranch(repo, name, start_point) => {
                let result = gitsail_application::CreateBranch::new(write.clone())
                    .execute(&repo, &name, start_point.as_ref());
                app.on_operation_finished(result)
            }
            Command::DeleteBranch(repo, name, force) => {
                let result =
                    gitsail_application::DeleteBranch::new(write.clone()).execute(&repo, &name, force);
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
            other => panic!("unexpected command: {other:?}"),
        };
        queue.extend(follow_up);
    }
}

fn current_branch(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn branch_exists(dir: &std::path::Path, name: &str) -> bool {
    std::process::Command::new("git")
        .args(["branch", "--list", name])
        .current_dir(dir)
        .output()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn creating_a_branch_via_the_tui_flow_creates_it_in_git() {
    let dir = TempDir::new("branch-create");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    app.update(Action::StartCreateBranch);
    for c in "feature/x".chars() {
        app.update(Action::BranchNameInput(c));
    }
    app.update(Action::Activate); // types -> confirming
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::CreateBranch { .. })
    ));

    let commands = app.update(Action::Activate); // confirming -> dispatch
    run_mutation(&mut app, commands);

    assert!(branch_exists(dir.path(), "feature/x"));
    assert!(matches!(app.operation(), OperationState::Succeeded(_)));
}

#[test]
fn checking_out_a_branch_switches_head() {
    let dir = TempDir::new("branch-checkout");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "develop"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    // Sidebar order matches `ListBranches`; find "develop" instead of
    // assuming a position.
    let target_index = app
        .filtered_branches()
        .iter()
        .position(|b| b.name.as_str() == "develop")
        .expect("develop must be listed");
    for _ in 0..target_index {
        app.update(Action::MoveDown);
    }

    app.update(Action::RequestCheckout);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::SwitchBranch { .. })
    ));

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert_eq!(current_branch(dir.path()), "develop");
    assert!(matches!(app.operation(), OperationState::Succeeded(_)));
}

#[test]
fn deleting_a_merged_branch_removes_it() {
    let dir = TempDir::new("branch-delete-merged");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "merged-topic"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    let target_index = app
        .filtered_branches()
        .iter()
        .position(|b| b.name.as_str() == "merged-topic")
        .expect("merged-topic must be listed");
    for _ in 0..target_index {
        app.update(Action::MoveDown);
    }

    app.update(Action::RequestDeleteBranch);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(!branch_exists(dir.path(), "merged-topic"));
}

#[test]
fn deleting_the_current_branch_fails_without_discarding_state() {
    let dir = TempDir::new("branch-delete-current-fails");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    // "main" is both the only branch and the current one; `is_current`
    // guards `request_delete_branch` itself, so drive the confirmation
    // directly to exercise the Core's own refusal instead.
    app.update(Action::RequestDeleteBranch);
    assert!(
        app.operation().is_idle(),
        "the app-level guard must refuse to even confirm deleting the current branch"
    );

    let repo = app.session().unwrap().repository().clone();
    let name = app.branches()[0].name.clone();
    let result = gitsail_application::DeleteBranch::new(write_port()).execute(&repo, &name, false);
    assert!(result.is_err(), "Git itself must refuse to delete the current branch");

    assert!(branch_exists(dir.path(), "main"));
    assert_eq!(current_branch(dir.path()), "main");
}

#[test]
fn sidebar_distinguishes_local_and_remote_branches() {
    let origin_dir = TempDir::new("branch-remote-origin");
    git(origin_dir.path(), &["init", "--quiet", "--bare", "--initial-branch=main"]);

    let dir = TempDir::new("branch-remote-clone");
    git(
        std::env::temp_dir().as_path(),
        &[
            "clone",
            "--quiet",
            origin_dir.path().to_str().unwrap(),
            dir.path().to_str().unwrap(),
        ],
    );
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "--quiet", "-m", "initial commit"]);
    git(dir.path(), &["push", "--quiet", "origin", "main"]);
    git(dir.path(), &["branch", "-r"]); // sanity: remote-tracking ref exists

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    assert!(
        app.branches()
            .iter()
            .any(|b| matches!(b.kind, gitsail_domain::BranchKind::Remote { .. })),
        "a remote-tracking branch must be listed distinctly from local ones: {:?}",
        app.branches()
    );
}
