//! Integration tests for T-235/US-083 (rebase, skip) against real, temporary
//! Git repositories via `GitCliProvider` (never a mock) — mirroring
//! `tests/merge_conflicts.rs`'s own fixture and `run_mutation` drive-loop
//! conventions.

mod support;

use gitsail_application::{GetRepositoryStatus, ListBranches, RebaseResult};
use gitsail_domain::{ErrorCode, InProgressOperation};
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
            Command::PlanRebase(repo, onto) => {
                let result =
                    gitsail_application::PlanRebase::new(write.clone()).execute(&repo, &onto);
                app.on_rebase_plan_loaded(result);
                Vec::new()
            }
            Command::ExecuteRebasePlan(repo, plan) => {
                let result = gitsail_application::ExecuteRebasePlan::new(write.clone())
                    .execute(&repo, &plan);
                app.on_rebase_finished(result)
            }
            Command::SkipOperation(repo) => {
                let result = gitsail_application::SkipOperation::new(write.clone()).execute(&repo);
                app.on_operation_resolution_finished(result)
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

/// Two independent feature commits diverging from `main` by one commit of
/// its own, on unrelated files throughout — so a plain rebase of the whole
/// range never conflicts, leaving the plan's own reordering/action
/// assignment as the only thing under test (T-236/US-084).
fn setup_two_commit_divergence(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature A"]);
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature B"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("main.txt"), "main\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "main advances"]);

    git(dir, &["checkout", "-q", "feature"]);
}

fn commit_subjects(dir: &std::path::Path, count: usize) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["log", &format!("-{count}"), "--pretty=format:%s"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect()
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
    assert!(matches!(
        app.last_rebase_result(),
        Some(RebaseResult::Conflict { .. })
    ));

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

// -- T-236/US-084: interactive rebase plan -----------------------------

/// T-236/US-084 criterion 1: opening the plan overlay lists the exact
/// candidate range `plan_rebase` returns, oldest first, each defaulted to
/// `Pick` — before anything is confirmed.
#[test]
fn requesting_a_rebase_plan_lists_candidates_oldest_first_defaulted_to_pick() {
    use gitsail_application::RebaseAction;

    let dir = TempDir::new("rebase-plan-list");
    setup_two_commit_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    let commands = app.update(Action::RequestRebasePlan);
    assert!(app.rebase_plan_open());
    run_mutation(&mut app, commands);

    let plan = app.rebase_plan().expect("plan should have loaded");
    assert_eq!(plan.onto_revision, "main");
    let subjects: Vec<_> = plan.entries.iter().map(|e| e.subject.clone()).collect();
    assert_eq!(subjects, vec!["feature A", "feature B"]);
    assert!(plan.entries.iter().all(|e| e.action == RebaseAction::Pick));
    assert!(plan.entries.iter().all(|e| e.message_override.is_none()));
}

/// T-236/US-084 criterion 1: reordering and reassigning an action, then
/// confirming, actually reapplies the commits in the new order with the
/// new action's effect — not just a state transition, a real resulting
/// tree.
#[test]
fn reordering_and_rewording_a_rebase_plan_reapplies_commits_in_the_new_order() {
    let dir = TempDir::new("rebase-plan-reorder-reword");
    setup_two_commit_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    let commands = app.update(Action::RequestRebasePlan);
    run_mutation(&mut app, commands);
    assert_eq!(app.rebase_plan().unwrap().entries.len(), 2);
    assert_eq!(app.rebase_plan_cursor(), 0);

    // Move the cursor to "feature B", then move that entry up a position —
    // the plan is now [feature B, feature A].
    app.update(Action::MoveDown);
    assert_eq!(app.rebase_plan_cursor(), 1);
    app.update(Action::RebasePlanMoveEntryUp);
    assert_eq!(app.rebase_plan_cursor(), 0);
    {
        let plan = app.rebase_plan().unwrap();
        let subjects: Vec<_> = plan.entries.iter().map(|e| e.subject.clone()).collect();
        assert_eq!(subjects, vec!["feature B", "feature A"]);
    }

    // Cycle the highlighted entry (now "feature B") to Reword — this opens
    // the message prompt pre-filled with its own subject.
    app.update(Action::RebasePlanCycleAction);
    assert_eq!(
        app.rebase_plan_reword_input(),
        Some("feature B"),
        "the reword prompt must pre-fill with the commit's own subject"
    );

    // Clear the pre-filled text and type a fresh message.
    for _ in 0.."feature B".len() {
        app.update(Action::RebasePlanRewordBackspace);
    }
    for c in "reworded B".chars() {
        app.update(Action::RebasePlanRewordInput(c));
    }
    app.update(Action::Activate); // submits the reword prompt
    assert!(app.rebase_plan_reword_input().is_none());
    assert_eq!(
        app.rebase_plan().unwrap().entries[0]
            .message_override
            .as_deref(),
        Some("reworded B")
    );

    // First `Enter` validates client-side and starts confirmation; the
    // second actually dispatches (mirrors the plain-rebase flow).
    app.update(Action::Activate);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::ExecuteRebasePlan {
            commit_count: 2,
            ..
        })
    ));
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    match app.last_rebase_result() {
        Some(RebaseResult::Completed { new_head }) => {
            assert_eq!(new_head.as_str(), current_head(dir.path()));
        }
        other => panic!("expected Some(Completed), got {other:?}"),
    }
    assert!(
        !app.rebase_plan_open(),
        "the plan overlay must close once the plan is dispatched"
    );
    assert!(app.rebase_plan().is_none());

    // The resulting history: reworded "feature B" (reapplied first),
    // followed by the untouched "feature A" at the tip.
    let subjects = commit_subjects(dir.path(), 2);
    assert_eq!(subjects, vec!["feature A", "reworded B"]);
    assert!(dir.path().join("a.txt").exists());
    assert!(dir.path().join("b.txt").exists());
    assert!(dir.path().join("main.txt").exists());
}

/// T-236/US-084 criterion 2 / T-237/US-085 criterion 2: assigning `Squash`
/// to the first entry is refused client-side, before ever reaching
/// [`Command::ExecuteRebasePlan`] — mirrors `RebasePlan::validate`'s own
/// rule exactly, and never silently drops or reorders anything to route
/// around it.
#[test]
fn squash_on_the_first_entry_is_refused_before_confirming() {
    let dir = TempDir::new("rebase-plan-squash-first-refused");
    setup_two_commit_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    let commands = app.update(Action::RequestRebasePlan);
    run_mutation(&mut app, commands);

    // Pick -> Reword -> Squash: two cycles lands on Squash at position 0.
    app.update(Action::RebasePlanCycleAction);
    // Landing on Reword opened the message prompt; escape it without
    // committing a message before cycling further.
    app.update(Action::Dismiss);
    app.update(Action::RebasePlanCycleAction);

    let commands = app.update(Action::Activate);
    assert!(
        commands.is_empty(),
        "an invalid plan must never reach a Command"
    );
    assert!(
        matches!(app.operation(), OperationState::Idle),
        "client-side validation must refuse before Confirming starts"
    );
    let error = app
        .rebase_plan_error()
        .expect("an invalid plan must surface a clear error");
    assert_eq!(error.code(), ErrorCode::InvalidRepositoryState);
    assert!(error.to_string().contains("squash"));

    // The plan itself is left untouched — no magic fix-up, no discarded
    // entries — so the overlay can still be corrected and retried.
    assert!(app.rebase_plan_open());
    assert_eq!(app.rebase_plan().unwrap().entries.len(), 2);
}

/// Escaping the Reword prompt cancels only the message edit — the entry's
/// action stays `Reword` rather than silently reverting, matching every
/// other text prompt's own "discard the unsubmitted edit" convention.
#[test]
fn escaping_the_reword_prompt_discards_only_the_unsubmitted_text() {
    use gitsail_application::RebaseAction;

    let dir = TempDir::new("rebase-plan-reword-escape");
    setup_two_commit_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    let commands = app.update(Action::RequestRebasePlan);
    run_mutation(&mut app, commands);

    app.update(Action::RebasePlanCycleAction);
    assert!(app.rebase_plan_reword_input().is_some());
    app.update(Action::RebasePlanRewordInput('x'));

    app.update(Action::Dismiss);
    assert!(app.rebase_plan_reword_input().is_none());
    assert!(
        app.rebase_plan_open(),
        "escaping the reword prompt must not close the whole plan overlay"
    );
    assert_eq!(
        app.rebase_plan().unwrap().entries[0].action,
        RebaseAction::Reword
    );
    assert!(app.rebase_plan().unwrap().entries[0]
        .message_override
        .is_none());

    // A second Esc closes the plan overlay outright.
    app.update(Action::Dismiss);
    assert!(!app.rebase_plan_open());
    assert!(app.rebase_plan().is_none());
}

/// T-236/US-084 criterion 2: a plan built against one state of `onto`
/// that has since moved is refused by the Core's own revalidation with a
/// clear error, never magically rebuilt or silently executed against the
/// newer state.
#[test]
fn a_stale_plan_is_refused_clearly_rather_than_silently_rebuilt() {
    let dir = TempDir::new("rebase-plan-stale");
    setup_two_commit_divergence(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "main");
    let commands = app.update(Action::RequestRebasePlan);
    run_mutation(&mut app, commands);
    assert!(app.rebase_plan().is_some());

    // `main` moves after the plan was built, without the app's knowledge —
    // mirrors another terminal/process advancing it concurrently.
    git(dir.path(), &["checkout", "-q", "main"]);
    std::fs::write(dir.path().join("late.txt"), "late\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "--quiet", "-m", "late main commit"]);
    git(dir.path(), &["checkout", "-q", "feature"]);

    app.update(Action::Activate); // client-side validate passes, begins Confirming
    let commands = app.update(Action::Activate); // confirms and dispatches
    run_mutation(&mut app, commands);

    match app.operation() {
        OperationState::Failed(OperationKind::ExecuteRebasePlan { .. }, error) => {
            assert_eq!(error.code(), ErrorCode::OperationConflict);
            assert!(error
                .to_string()
                .contains("now resolves to a different commit"));
        }
        other => panic!("expected Some(Failed(ExecuteRebasePlan, ..)), got {other:?}"),
    }
    assert!(
        !app.rebase_plan_open(),
        "a failed execution must never leave a stale plan around to retry blindly"
    );
    assert!(app.rebase_plan().is_none());
}
