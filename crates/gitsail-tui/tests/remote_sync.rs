//! Integration tests for T-182/US-049 ("Sincronizar com remote pela TUI")
//! against real, temporary Git repositories via `GitCliProvider` (never a
//! mock) — every "remote" here is another local, on-disk *bare* repository,
//! mirroring `gitsail-git`'s own EPIC-19 integration tests and never
//! touching a real network.

mod support;

use gitsail_application::{GetRepositoryStatus, ListBranches, OpenRepository, PullOutcome};
use gitsail_tui::{ui, Action, App, Command, OperationKind, OperationState};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{clone_repo, git, init_bare_remote, init_repo_with_initial_commit, read_port, write_port, TempDir};

/// Opens `dir` and drains every command `on_repository_opened` issues
/// (status, branches, tags, remotes, stash, the first commit-graph page)
/// against the real adapter, so `app.remotes()` genuinely reflects the
/// repository's configured remote(s) before a test resolves a sync target —
/// exactly the data [`gitsail_tui::App`]'s own `resolve_sync_remote` reads.
fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let read = read_port();
    let repo = OpenRepository::new(read.clone()).execute(dir).unwrap();
    let commands = app.on_repository_opened(Ok(repo.clone()));

    let ticket = commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(read.clone()).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    for command in commands {
        match command {
            Command::LoadBranches(generation, repo) => {
                let branches = ListBranches::new(read.clone()).execute(&repo).unwrap();
                app.on_branches_loaded(generation, Ok(branches));
            }
            Command::LoadTags(generation, repo) => {
                app.on_tags_loaded(generation, read.list_tags(&repo));
            }
            Command::LoadRemotes(generation, repo) => {
                app.on_remotes_loaded(generation, read.list_remotes(&repo));
            }
            Command::LoadStashEntries(generation, repo) => {
                app.on_stash_entries_loaded(generation, read.list_stash_entries(&repo));
            }
            Command::LoadReflog(generation, repo) => {
                app.on_reflog_loaded(generation, read.reflog(&repo));
            }
            _ => {}
        }
    }
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — including chasing whatever follow-up refresh
/// commands a successful mutation returns, mirroring
/// `tests/branch_management.rs::run_mutation`.
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::Fetch(repo, remote) => {
                let result = gitsail_application::Fetch::new(write.clone()).execute(
                    &repo,
                    &remote,
                    &gitsail_domain::CancellationToken::new(),
                );
                app.on_operation_finished(result)
            }
            Command::Pull(repo, remote, branch) => {
                let result = gitsail_application::Pull::new(write.clone()).execute(
                    &repo,
                    &remote,
                    &branch,
                    &gitsail_domain::CancellationToken::new(),
                );
                app.on_pull_finished(result)
            }
            Command::Push(repo, remote, branch) => {
                let result = gitsail_application::Push::new(write.clone()).execute(
                    &repo,
                    &remote,
                    &branch,
                    &gitsail_domain::CancellationToken::new(),
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
            other => panic!("unexpected command: {other:?}"),
        };
        queue.extend(follow_up);
    }
}

fn remote_tracking_head(dir: &std::path::Path, remote: &str, branch: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", &format!("refs/remotes/{remote}/{branch}")])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn head(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    support::buffer_text(&terminal)
}

/// Seeds `dir` (a fresh clone of a bare remote) with an initial commit and
/// pushes it to `origin/main`, establishing the branch on both sides —
/// mirroring `gitsail-git`'s own EPIC-19 fixture flow. Every test below
/// needs this before treating a clone as "the local repository the app has
/// open": cloning a brand-new bare remote alone leaves `HEAD` unborn, with
/// no current branch to resolve a sync target against.
fn seed_and_push_initial_commit(dir: &std::path::Path) {
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "--quiet", "-m", "first commit"]);
    git(dir, &["push", "--quiet", "--", "origin", "main"]);
}

#[test]
fn requesting_fetch_shows_the_resolved_remote_before_dispatching() {
    let remote_dir = init_bare_remote("sync-fetch-remote");
    let local_dir = clone_repo(remote_dir.path(), "sync-fetch-local");
    seed_and_push_initial_commit(local_dir.path());

    let (mut app, _commands) = App::new(local_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, local_dir.path());

    assert_eq!(
        app.remotes().iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["origin"],
        "a fresh clone must report its single 'origin' remote"
    );

    let commands = app.update(Action::RequestFetch);
    // Fetch is Safe (SAD §20's own named example), so it dispatches
    // immediately rather than waiting for a confirmation keypress —
    // criterion 2: progress never blocks navigation. The remote it targets
    // is still explicit, shown right away in the operation overlay
    // (criterion 1), not only after the fact.
    let text = render(&app);
    assert!(
        text.contains("origin"),
        "the resolved remote must be shown before/while fetching:\n{text}"
    );
    match commands.as_slice() {
        [Command::Fetch(_, remote)] => assert_eq!(remote, "origin"),
        other => panic!("expected exactly one Fetch command, got {other:?}"),
    }

    run_mutation(&mut app, commands);
    assert!(
        app.operation().is_idle(),
        "a Safe operation's success must not linger as a state needing dismissal"
    );
    assert!(app.sync_error().is_none());
}

#[test]
fn fetch_updates_remote_tracking_refs_without_touching_the_working_tree() {
    let remote_dir = init_bare_remote("sync-fetch2-remote");
    let local_dir = clone_repo(remote_dir.path(), "sync-fetch2-local");
    seed_and_push_initial_commit(local_dir.path());

    // Someone else pushes a new commit to the remote after this clone.
    let other_dir = clone_repo(remote_dir.path(), "sync-fetch2-other");
    std::fs::write(other_dir.path().join("new.txt"), "content\n").unwrap();
    git(other_dir.path(), &["add", "new.txt"]);
    git(other_dir.path(), &["commit", "--quiet", "-m", "advance remote"]);
    git(other_dir.path(), &["push", "--quiet", "origin", "main"]);
    let advanced_head = head(other_dir.path());

    let (mut app, _commands) = App::new(local_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, local_dir.path());

    let before = head(local_dir.path());
    let commands = app.update(Action::RequestFetch);
    run_mutation(&mut app, commands);

    assert_eq!(
        remote_tracking_head(local_dir.path(), "origin", "main"),
        advanced_head,
        "fetch must update the remote-tracking ref to the new commit"
    );
    assert_eq!(
        head(local_dir.path()),
        before,
        "fetch must never touch the working tree/HEAD"
    );
}

#[test]
fn pulling_fast_forwards_a_behind_branch_and_reports_the_outcome() {
    let remote_dir = init_bare_remote("sync-pull-ff-remote");
    let behind_dir = clone_repo(remote_dir.path(), "sync-pull-ff-behind");
    seed_and_push_initial_commit(behind_dir.path());

    let ahead_dir = clone_repo(remote_dir.path(), "sync-pull-ff-ahead");
    std::fs::write(ahead_dir.path().join("new.txt"), "content\n").unwrap();
    git(ahead_dir.path(), &["add", "new.txt"]);
    git(ahead_dir.path(), &["commit", "--quiet", "-m", "advance"]);
    git(ahead_dir.path(), &["push", "--quiet", "origin", "main"]);
    let advanced_head = head(ahead_dir.path());

    let (mut app, _commands) = App::new(behind_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, behind_dir.path());

    app.update(Action::RequestPull);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Pull { .. })
    ));
    let text = render(&app);
    assert!(
        text.contains("main") && text.contains("origin"),
        "the branch and remote must both be explicit before pulling:\n{text}"
    );

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert_eq!(
        head(behind_dir.path()),
        advanced_head,
        "a fast-forward pull must move the local branch to the remote's tip"
    );
    assert_eq!(
        app.last_pull_outcome(),
        Some(&PullOutcome::FastForwarded {
            new_head: gitsail_domain::CommitHash::new(&advanced_head).unwrap()
        })
    );
    assert!(matches!(app.operation(), OperationState::Succeeded(_)));
}

#[test]
fn pulling_with_nothing_new_reports_already_up_to_date() {
    let remote_dir = init_bare_remote("sync-pull-uptodate-remote");
    let local_dir = clone_repo(remote_dir.path(), "sync-pull-uptodate-local");
    seed_and_push_initial_commit(local_dir.path());

    let (mut app, _commands) = App::new(local_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, local_dir.path());

    app.update(Action::RequestPull);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert_eq!(app.last_pull_outcome(), Some(&PullOutcome::AlreadyUpToDate));
}

#[test]
fn pushing_publishes_local_commits_to_the_remote() {
    let remote_dir = init_bare_remote("sync-push-remote");
    let local_dir = clone_repo(remote_dir.path(), "sync-push-local");

    std::fs::write(local_dir.path().join("new.txt"), "content\n").unwrap();
    git(local_dir.path(), &["add", "new.txt"]);
    git(local_dir.path(), &["commit", "--quiet", "-m", "local work"]);
    let new_head = head(local_dir.path());

    let (mut app, _commands) = App::new(local_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, local_dir.path());

    app.update(Action::RequestPush);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Push { .. })
    ));

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    let remote_head = std::process::Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(remote_dir.path())
        .output()
        .unwrap();
    let remote_head = String::from_utf8(remote_head.stdout).unwrap().trim().to_string();
    assert_eq!(
        remote_head, new_head,
        "the bare remote must now have the pushed commit"
    );
    assert!(matches!(app.operation(), OperationState::Succeeded(_)));
}

/// T-182's DoD explicitly requires this scenario: "E2E com remote fixture
/// cobre sincronização e push rejeitado."
#[test]
fn a_non_fast_forward_push_is_rejected_and_the_remote_state_is_preserved() {
    let remote_dir = init_bare_remote("sync-push-reject-remote");
    let seed_dir = clone_repo(remote_dir.path(), "sync-push-reject-seed");

    // Another clone pushes first, advancing the remote.
    let other_dir = clone_repo(remote_dir.path(), "sync-push-reject-other");
    std::fs::write(other_dir.path().join("other.txt"), "content\n").unwrap();
    git(other_dir.path(), &["add", "other.txt"]);
    git(other_dir.path(), &["commit", "--quiet", "-m", "other's commit"]);
    git(other_dir.path(), &["push", "--quiet", "origin", "main"]);
    let remote_head_before = std::process::Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(remote_dir.path())
        .output()
        .unwrap();
    let remote_head_before = String::from_utf8(remote_head_before.stdout)
        .unwrap()
        .trim()
        .to_string();

    // `seed_dir`, unaware of that push, commits on top of the old base —
    // pushing now would be a non-fast-forward.
    std::fs::write(seed_dir.path().join("mine.txt"), "content\n").unwrap();
    git(seed_dir.path(), &["add", "mine.txt"]);
    git(seed_dir.path(), &["commit", "--quiet", "-m", "my divergent commit"]);

    let (mut app, _commands) = App::new(seed_dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, seed_dir.path());

    app.update(Action::RequestPush);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(
        matches!(app.operation(), OperationState::Failed(_, _)),
        "a rejected push must surface as a Failed operation, got {:?}",
        app.operation()
    );
    let text = render(&app);
    assert!(
        text.contains("Nothing was changed") || text.contains("dismisses"),
        "a rejected push must never claim anything succeeded:\n{text}"
    );

    let remote_head_after = std::process::Command::new("git")
        .args(["rev-parse", "refs/heads/main"])
        .current_dir(remote_dir.path())
        .output()
        .unwrap();
    let remote_head_after = String::from_utf8(remote_head_after.stdout)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(
        remote_head_after, remote_head_before,
        "a rejected push must never be silently escalated to a force push"
    );
}

#[test]
fn fetch_with_no_remote_configured_shows_a_clear_error_instead_of_guessing() {
    let dir = TempDir::new("sync-no-remote");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    assert!(app.remotes().is_empty());

    let commands = app.update(Action::RequestFetch);
    assert!(commands.is_empty(), "nothing must be dispatched without a resolvable remote");
    assert!(app.sync_error().is_some());
    let text = render(&app);
    assert!(
        text.contains("no remote"),
        "the missing-remote error must be visible:\n{text}"
    );
}
