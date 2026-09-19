//! Integration tests for T-231/US-079 (execute a merge with an intent
//! preview), T-232/US-080 (guide conflict resolution) and T-233/US-081
//! (continue/abort a pending operation) against real, temporary Git
//! repositories via `GitCliProvider` (never a mock) — mirroring
//! `tests/branch_management.rs`'s own fixture and `run_mutation` drive-loop
//! conventions.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use gitsail_application::{GetRepositoryStatus, ListBranches, MergeResult, OpenRepository};
use gitsail_domain::InProgressOperation;
use gitsail_tui::{Action, App, Command, OperationKind, OperationState};
use support::{git, read_port, write_port, TempDir};

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
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port.clone()).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
    app.on_in_progress_operation_loaded(generation, port.detect_in_progress_operation(&repo));
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would — including chasing every follow-up command a
/// successful mutation returns, mirroring
/// `tests/branch_management.rs::run_mutation`, extended with this epic's
/// own new [`Command`] variants.
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
            Command::ContinueOperation(repo) => {
                let result =
                    gitsail_application::ContinueOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
            }
            Command::AbortOperation(repo) => {
                let result = gitsail_application::AbortOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
            }
            Command::MarkConflictResolved(repo, path) => {
                let result = gitsail_application::MarkConflictResolved::new(write.clone())
                    .execute(&repo, &path);
                app.on_conflict_resolution_finished(result)
            }
            Command::TakeConflictSide(repo, path, side) => {
                let result = gitsail_application::TakeConflictSide::new(write.clone())
                    .execute(&repo, &path, side);
                app.on_conflict_resolution_finished(result)
            }
            Command::LoadConflictSides(repo, path) => {
                let result = read.conflict_sides(&repo, &path);
                app.on_conflict_sides_loaded(path, result);
                Vec::new()
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

/// Sets up two branches (`main`/`feature`) with a non-conflicting divergence:
/// `main` gains a commit to `a.txt`, `feature` gains a commit to `b.txt`.
fn setup_non_conflicting_divergence(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("b.txt"), "b\nfeature change\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "a\nmain change\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "main change"]);
}

/// Sets up two branches that both modify the same line of the same file, so
/// merging one into the other reliably conflicts — mirrors
/// `gitsail-git/tests/t231_233_merge_conflicts.rs::setup_diverging_branches`.
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
fn merging_a_reference_creates_a_merge_commit_via_the_tui_flow() {
    let dir = TempDir::new("merge-commit");
    setup_non_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Merge { target }) if target == "feature"
    ));

    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    match app.last_merge_result() {
        Some(MergeResult::MergeCommitCreated { hash }) => {
            assert_eq!(hash.as_str(), current_head(dir.path()));
        }
        other => panic!("expected Some(MergeCommitCreated), got {other:?}"),
    }
    assert!(app.in_progress_operation().is_none());
}

/// T-255/US-122 criterion 3 ("cancelamento"): the same guarantee
/// `src/app.rs::dismissing_a_pending_destructive_reset_confirmation_dispatches_nothing`
/// already covers for a `Reset` confirmation, exercised end to end here for
/// a real merge against a real repository — declining the confirmation
/// prompt (`Esc`/[`Action::Dismiss`]) must dispatch nothing and leave HEAD
/// exactly where it was (Destructive Operations & Confirmation Guardrails
/// wiki rule 5: "Cancelling before confirmation leaves the repository
/// completely untouched").
#[test]
fn declining_a_pending_merge_confirmation_dispatches_nothing_and_leaves_head_untouched() {
    let dir = TempDir::new("merge-decline-confirmation");
    setup_non_conflicting_divergence(dir.path());
    let head_before = current_head(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::Merge { target }) if target == "feature"
    ));

    let commands = app.update(Action::Dismiss);
    assert!(
        commands.is_empty(),
        "declining a merge confirmation must dispatch nothing: {commands:?}"
    );
    assert!(
        app.operation().is_idle(),
        "declining a confirmation must cancel it, never silently confirm it"
    );
    assert_eq!(
        current_head(dir.path()),
        head_before,
        "HEAD must be completely untouched after cancelling"
    );
    assert!(app.last_merge_result().is_none());
    assert!(app.in_progress_operation().is_none());
}

#[test]
fn merging_a_conflicting_reference_opens_a_conflicts_overlay_that_resolves_and_continues() {
    let dir = TempDir::new("merge-conflict-resolve");
    setup_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    // T-231 criterion 2/3: a conflict is its own distinct, explicit result —
    // never a bare success — and the repository is genuinely left with a
    // pending merge for T-232/T-233 to pick up.
    match app.last_merge_result() {
        Some(MergeResult::Conflict { files }) => assert_eq!(files.len(), 1),
        other => panic!("expected Some(Conflict), got {other:?}"),
    }
    assert!(matches!(
        app.in_progress_operation(),
        InProgressOperation::Merge(_)
    ));

    // T-232 criterion 1: the overlay lists the conflicted file.
    app.update(Action::ToggleConflictsPanel);
    assert!(app.conflicts_open());
    assert_eq!(app.in_progress_operation().conflicted_files().len(), 1);

    // T-232 criterion 2: the base/ours/theirs sides can be inspected.
    let commands = app.update(Action::InspectConflict);
    run_mutation(&mut app, commands);
    let sides = app.inspected_conflict().expect("conflict sides must load");
    assert!(matches!(
        sides.ours,
        gitsail_domain::ConflictSideContent::Text(_)
    ));
    assert!(matches!(
        sides.theirs,
        gitsail_domain::ConflictSideContent::Text(_)
    ));

    // T-232 criterion 3: resolving is only ever this explicit action —
    // simulates resolving the conflict by editing the file (as if outside
    // GitSail) then marking it resolved.
    std::fs::write(dir.path().join("f.txt"), "line1\nRESOLVED\nline3\n").unwrap();
    let commands = app.update(Action::MarkConflictResolved);
    run_mutation(&mut app, commands);

    // T-233 criterion 2: continue re-checks there is nothing left
    // conflicted, then completes the merge commit.
    app.update(Action::RequestContinueOperation);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::ContinueOperation)
    ));
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(
        app.in_progress_operation().is_none(),
        "T-233 criterion 3: the real resulting state must be reinspected, not presumed"
    );
    let head = current_head(dir.path());
    let parents_output = std::process::Command::new("git")
        .args(["rev-parse", &format!("{head}^1"), &format!("{head}^2")])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        parents_output.status.success(),
        "HEAD must be a two-parent merge commit after continue"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "line1\nRESOLVED\nline3\n"
    );
}

#[test]
fn a_binary_conflict_resolves_via_take_conflict_side() {
    let dir = TempDir::new("merge-conflict-binary");
    init_repo(dir.path());
    std::fs::write(dir.path().join("img.bin"), [0u8, 1, 2, 3]).unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "base"]);

    git(dir.path(), &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.path().join("img.bin"), [0u8, 9, 9, 9]).unwrap();
    git(dir.path(), &["add", "-A"]);
    git(
        dir.path(),
        &["commit", "--quiet", "-m", "feature binary change"],
    );

    git(dir.path(), &["checkout", "-q", "main"]);
    std::fs::write(dir.path().join("img.bin"), [0u8, 5, 5, 5]).unwrap();
    git(dir.path(), &["add", "-A"]);
    git(
        dir.path(),
        &["commit", "--quiet", "-m", "main binary change"],
    );

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(matches!(
        app.last_merge_result(),
        Some(MergeResult::Conflict { .. })
    ));

    app.update(Action::ToggleConflictsPanel);
    let commands = app.update(Action::TakeConflictSideTheirs);
    run_mutation(&mut app, commands);

    assert_eq!(
        std::fs::read(dir.path().join("img.bin")).unwrap(),
        vec![0u8, 9, 9, 9]
    );

    app.update(Action::RequestContinueOperation);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(app.in_progress_operation().is_none());
    assert_eq!(
        std::fs::read(dir.path().join("img.bin")).unwrap(),
        vec![0u8, 9, 9, 9]
    );
}

#[test]
fn aborting_a_pending_merge_restores_head_and_preserves_unrelated_work() {
    let dir = TempDir::new("merge-abort");
    setup_conflicting_divergence(dir.path());
    let head_before_merge = current_head(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    // Unrelated, unstaged local work that must survive the abort untouched.
    std::fs::write(dir.path().join("unrelated.txt"), "unrelated work\n").unwrap();

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(matches!(
        app.last_merge_result(),
        Some(MergeResult::Conflict { .. })
    ));

    app.update(Action::ToggleConflictsPanel);
    app.update(Action::RequestAbortOperation);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::AbortOperation)
    ));
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(
        app.in_progress_operation().is_none(),
        "T-233 criterion 3: the real resulting state must be reinspected, not presumed"
    );
    assert_eq!(current_head(dir.path()), head_before_merge);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("unrelated.txt")).unwrap(),
        "unrelated work\n",
        "abort must never discard unrelated local work"
    );
}

/// T-267 (GitHub issue #1): the confirmation overlay is modal, so `Esc`
/// while it is up must cancel *it* — not the conflicts overlay rendered
/// underneath it. `App::dismiss` used to walk an else-chain in which
/// `conflicts_open` was tested before the operation, so the overlay that
/// closed was the one the person was not even looking at.
#[test]
fn esc_over_the_conflicts_overlay_cancels_the_confirmation_and_keeps_the_conflicts_open() {
    let dir = TempDir::new("merge-conflict-esc-priority");
    setup_conflicting_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    app.update(Action::ToggleConflictsPanel);
    assert!(app.conflicts_open());

    // `a` inside the conflicts overlay asks to abort the pending merge.
    app.update(Action::RequestAbortOperation);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::AbortOperation)
    ));
    assert_eq!(
        gitsail_tui::keymap::action_for(
            KeyEvent {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            app.input_context()
        ),
        Some(Action::Dismiss),
        "Esc must have a meaning while the confirmation is up"
    );

    let commands = app.update(Action::Dismiss);
    assert!(
        commands.is_empty(),
        "cancelling must dispatch nothing: {commands:?}"
    );
    assert!(
        app.operation().is_idle(),
        "Esc must cancel the confirmation itself"
    );
    assert!(
        app.conflicts_open(),
        "the conflicts overlay underneath must stay open"
    );
    assert!(matches!(
        app.in_progress_operation(),
        InProgressOperation::Merge(_)
    ));
}
