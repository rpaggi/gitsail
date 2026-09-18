//! Integration tests for T-235/US-083 (`RepositoryWritePort::rebase`/
//! `skip_operation`), T-236/US-084 (`RepositoryWritePort::plan_rebase`/
//! `execute_rebase_plan`), and T-237/US-085 (squash/fixup as plan actions)
//! against real, temporary Git repositories — mirroring
//! `tests/t231_233_merge_conflicts.rs`'s own fixture conventions.
//!
//! Every scenario exercises real `git rebase`/`git rebase -i` behavior end
//! to end: completion and conflict are always asserted as distinct
//! [`RebaseResult`] variants, and the interactive-plan mechanism
//! (`GIT_SEQUENCE_EDITOR`) is exercised against real commits — including one
//! whose subject/message is deliberately crafted to look like a shell
//! injection attempt (the security-critical test T-236/US-084 criterion 3
//! requires).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{RebaseAction, RebaseResult, RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::{CommitHash, ErrorCode, InProgressOperation};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

/// A uniquely named temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-rebase-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `git <args>` directly (test fixture setup only — production code in
/// this crate must always go through `GitProcessRunner`).
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"))
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git_ok(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git_ok(dir.path(), &["config", "user.name", "Test User"]);
    git_ok(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn commit_all(dir: &Path, message: &str) {
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "--quiet", "-m", message]);
}

fn rev_parse(dir: &Path, revision: &str) -> CommitHash {
    let output = git(dir, &["rev-parse", revision]);
    assert!(output.status.success(), "git rev-parse {revision} failed");
    let hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
    CommitHash::new(hash).unwrap()
}

fn commit_subject(dir: &Path, revision: &str) -> String {
    let output = git(dir, &["log", "-1", "--pretty=format:%s", revision]);
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn commit_body(dir: &Path, revision: &str) -> String {
    let output = git(dir, &["log", "-1", "--pretty=format:%B", revision]);
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

// ---------------------------------------------------------------------
// T-235/US-083: rebase — success, conflict, dirty tree, pending op.
// ---------------------------------------------------------------------

#[test]
fn rebase_reapplies_commits_cleanly_onto_the_new_base() {
    let repo_dir = init_repo("rebase-clean");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "feature.txt", "feature\n");
    commit_all(repo_dir.path(), "feature work");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "main advances");
    let main_tip = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "feature"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::rebase(&provider, &repo, "main").unwrap();

    let new_head = match result {
        RebaseResult::Completed { new_head } => new_head,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), new_head);
    assert_eq!(rev_parse(repo_dir.path(), "HEAD~1"), main_tip);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

fn setup_conflicting_rebase(repo_dir: &Path) {
    write_file(repo_dir, "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir, "base");

    git_ok(repo_dir, &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir, "f.txt", "line1\nFEATURE\nline3\n");
    commit_all(repo_dir, "feature change");

    git_ok(repo_dir, &["checkout", "-q", "main"]);
    write_file(repo_dir, "f.txt", "line1\nMAIN\nline3\n");
    commit_all(repo_dir, "main change");

    git_ok(repo_dir, &["checkout", "-q", "feature"]);
}

#[test]
fn rebase_reports_a_conflict_distinctly_and_leaves_a_pending_rebase_for_recovery() {
    let repo_dir = init_repo("rebase-conflict");
    setup_conflicting_rebase(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::rebase(&provider, &repo, "main").unwrap();

    match result {
        RebaseResult::Conflict { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, PathBuf::from("f.txt"));
        }
        other => panic!("a conflict must never be reported as {other:?}"),
    }
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Rebase(_)
    ));

    // Recovery via abort — reusing T-233's shared workflow unchanged.
    RepositoryWritePort::abort_operation(&provider, &repo).unwrap();
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn rebase_conflict_recovers_via_continue_after_resolving() {
    let repo_dir = init_repo("rebase-conflict-continue");
    setup_conflicting_rebase(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::rebase(&provider, &repo, "main").unwrap();
    assert!(matches!(result, RebaseResult::Conflict { .. }));

    write_file(repo_dir.path(), "f.txt", "line1\nRESOLVED\nline3\n");
    RepositoryWritePort::mark_conflict_resolved(&provider, &repo, Path::new("f.txt")).unwrap();
    RepositoryWritePort::continue_operation(&provider, &repo).unwrap();

    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "line1\nRESOLVED\nline3\n"
    );
}

#[test]
fn rebase_conflict_recovers_via_skip() {
    let repo_dir = init_repo("rebase-conflict-skip");
    setup_conflicting_rebase(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::rebase(&provider, &repo, "main").unwrap();
    assert!(matches!(result, RebaseResult::Conflict { .. }));

    RepositoryWritePort::skip_operation(&provider, &repo).unwrap();

    // The conflicting commit was skipped entirely: the rebase is done, and
    // the working tree holds `main`'s content, not `feature`'s.
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "line1\nMAIN\nline3\n"
    );
}

#[test]
fn rebase_refuses_a_dirty_working_tree_without_ever_stashing_automatically() {
    let repo_dir = init_repo("rebase-dirty");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "base");
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "feature.txt", "feature\n");
    commit_all(repo_dir.path(), "feature work");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "main advances");
    git_ok(repo_dir.path(), &["checkout", "-q", "feature"]);

    // Uncommitted local change.
    write_file(repo_dir.path(), "dirty.txt", "uncommitted\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::rebase(&provider, &repo, "main").unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    // Never touched: no rebase started, no stash was created behind the
    // back of the caller.
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    let stash_list = git(repo_dir.path(), &["stash", "list"]);
    assert!(String::from_utf8(stash_list.stdout).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("dirty.txt")).unwrap(),
        "uncommitted\n"
    );
}

#[test]
fn rebase_refuses_to_start_when_another_operation_is_already_in_progress() {
    let repo_dir = init_repo("rebase-refuses-existing-op");
    setup_conflicting_rebase(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // Leave a real conflicting merge pending.
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let err = RepositoryWritePort::rebase(&provider, &repo, "feature").unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn skip_operation_is_refused_with_a_clear_unsupported_message_for_a_merge() {
    let repo_dir = init_repo("skip-refuses-merge");
    setup_conflicting_rebase(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let err = RepositoryWritePort::skip_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert!(err.to_string().to_lowercase().contains("unsupported"));

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn skip_operation_refuses_when_nothing_is_pending() {
    let repo_dir = init_repo("skip-refuses-nothing-pending");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::skip_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

// ---------------------------------------------------------------------
// T-236/US-084: plan_rebase / execute_rebase_plan — reorder, precondition
// revalidation, and the security-critical sequence-editor mechanism.
// ---------------------------------------------------------------------

#[test]
fn plan_rebase_lists_candidate_commits_oldest_first_defaulted_to_pick() {
    let repo_dir = init_repo("plan-lists-oldest-first");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");

    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "second");
    write_file(repo_dir.path(), "c.txt", "c\n");
    commit_all(repo_dir.path(), "third");
    let third = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let plan = provider.plan_rebase(&repo, base.as_str()).unwrap();

    assert_eq!(plan.onto, base);
    assert_eq!(plan.branch_head, third);
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].subject, "second");
    assert_eq!(plan.entries[1].subject, "third");
    assert!(plan
        .entries
        .iter()
        .all(|e| e.action == RebaseAction::Pick && e.message_override.is_none()));

    // Read-only: nothing changed.
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), third);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn execute_rebase_plan_applies_a_plain_pick_only_plan_unchanged() {
    let repo_dir = init_repo("execute-plain-picks");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "second");
    write_file(repo_dir.path(), "c.txt", "c\n");
    commit_all(repo_dir.path(), "third");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let plan = provider.plan_rebase(&repo, base.as_str()).unwrap();

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();

    let new_head = match result {
        RebaseResult::Completed { new_head } => new_head,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), new_head);
    assert_eq!(commit_subject(repo_dir.path(), "HEAD"), "third");
    assert_eq!(commit_subject(repo_dir.path(), "HEAD~1"), "second");
    assert_eq!(commit_subject(repo_dir.path(), "HEAD~2"), "base");
}

#[test]
fn execute_rebase_plan_reorders_commits_per_the_plans_own_entry_order() {
    let repo_dir = init_repo("execute-reorders");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "alpha");
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "beta");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    assert_eq!(plan.entries[0].subject, "alpha");
    assert_eq!(plan.entries[1].subject, "beta");
    plan.entries.swap(0, 1);

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    assert!(matches!(result, RebaseResult::Completed { .. }));

    assert_eq!(commit_subject(repo_dir.path(), "HEAD"), "alpha");
    assert_eq!(commit_subject(repo_dir.path(), "HEAD~1"), "beta");
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap(),
        "a\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("b.txt")).unwrap(),
        "b\n"
    );
}

#[test]
fn execute_rebase_plan_applies_a_reword_via_amend_never_opening_an_editor() {
    let repo_dir = init_repo("execute-reword");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "original message");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    plan.entries[0].action = RebaseAction::Reword;
    plan.entries[0].message_override = Some("a much better message".to_string());

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    assert!(matches!(result, RebaseResult::Completed { .. }));

    assert_eq!(
        commit_subject(repo_dir.path(), "HEAD"),
        "a much better message"
    );
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn execute_rebase_plan_squash_combines_both_messages_and_fixup_discards_the_folded_one() {
    let repo_dir = init_repo("execute-squash-fixup");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "keep-me");
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "squash-me");
    write_file(repo_dir.path(), "c.txt", "c\n");
    commit_all(repo_dir.path(), "fixup-me");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    assert_eq!(plan.entries.len(), 3);
    plan.entries[1].action = RebaseAction::Squash;
    plan.entries[2].action = RebaseAction::Fixup;

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    assert!(matches!(result, RebaseResult::Completed { .. }));

    // Everything folded into one commit on top of base.
    assert_eq!(rev_parse(repo_dir.path(), "HEAD~1"), base);

    let final_message = commit_body(repo_dir.path(), "HEAD");
    // Squash keeps *both* messages, combined.
    assert!(final_message.contains("keep-me"));
    assert!(final_message.contains("squash-me"));
    // Fixup discards the folded commit's own message outright.
    assert!(!final_message.contains("fixup-me"));

    for file in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            repo_dir.path().join(file).exists(),
            "{file} must survive the fold"
        );
    }
}

#[test]
fn execute_rebase_plan_rejects_squash_at_the_first_position_before_touching_anything() {
    let repo_dir = init_repo("execute-rejects-first-squash");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "only-commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    plan.entries[0].action = RebaseAction::Squash;
    let head_before = rev_parse(repo_dir.path(), "HEAD");

    let err = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), head_before);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn execute_rebase_plan_conflict_recovers_via_continue() {
    let repo_dir = init_repo("execute-conflict-continue");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");

    write_file(repo_dir.path(), "unrelated.txt", "1\n");
    commit_all(repo_dir.path(), "unrelated change");
    write_file(repo_dir.path(), "f.txt", "line1\nFEATURE\nline3\n");
    commit_all(repo_dir.path(), "feature change");

    // Advance `main` past `base` with a conflicting edit to the same line.
    git_ok(repo_dir.path(), &["branch", "main-target", base.as_str()]);
    git_ok(repo_dir.path(), &["checkout", "-q", "main-target"]);
    write_file(repo_dir.path(), "f.txt", "line1\nMAIN\nline3\n");
    commit_all(repo_dir.path(), "main change");
    let new_base = rev_parse(repo_dir.path(), "HEAD");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let plan = provider.plan_rebase(&repo, new_base.as_str()).unwrap();
    assert_eq!(plan.entries.len(), 2);

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    assert!(matches!(result, RebaseResult::Conflict { .. }));
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Rebase(_)
    ));

    write_file(repo_dir.path(), "f.txt", "line1\nRESOLVED\nline3\n");
    RepositoryWritePort::mark_conflict_resolved(&provider, &repo, Path::new("f.txt")).unwrap();
    RepositoryWritePort::continue_operation(&provider, &repo).unwrap();

    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    assert!(repo_dir.path().join("unrelated.txt").exists());
}

/// T-236/US-084 criterion 2: a plan built against a base that has since
/// moved must never silently execute against the new state.
#[test]
fn execute_rebase_plan_refuses_when_onto_has_moved_since_the_plan_was_built() {
    let repo_dir = init_repo("execute-stale-onto");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "feature work");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let plan = provider.plan_rebase(&repo, "main").unwrap();

    // `main` (the base the plan was built against) advances after the plan
    // was built, before it is executed.
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "main moved on");

    let err = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

/// T-236/US-084 criterion 2: a plan built against an old `HEAD` must never
/// silently execute against a newer one either.
#[test]
fn execute_rebase_plan_refuses_when_head_has_moved_since_the_plan_was_built() {
    let repo_dir = init_repo("execute-stale-head");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "first");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let plan = provider.plan_rebase(&repo, base.as_str()).unwrap();

    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "second, after the plan was built");

    let err = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);
}

/// Security-critical (T-236/US-084 criterion 3): a commit whose subject and
/// a `Reword` message are both crafted to look like shell injection attempts
/// must never result in anything beyond the plain, expected rebase — no
/// stray file is ever created, and the malicious text ends up exactly where
/// it is supposed to (as literal, inert data), never executed.
#[test]
fn execute_rebase_plan_never_executes_malicious_commit_text_as_a_shell_command() {
    let repo_dir = init_repo("execute-injection-safety");
    let marker = repo_dir.path().join("PWNED_MARKER");
    let marker_display = marker.display();

    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");

    // A commit whose subject is itself a shell-injection attempt: if the
    // sequence-editor mechanism ever interpolated this into a shell command
    // instead of treating it as inert data, this `touch` would run.
    write_file(repo_dir.path(), "a.txt", "a\n");
    let evil_subject = format!("evil\"; touch {marker_display}; echo pwned #");
    commit_all(repo_dir.path(), &evil_subject);

    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "second commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].subject, evil_subject);

    // Also reword the second commit with a message crafted the same way —
    // exercised through the `git commit --amend -m <text>` path.
    plan.entries[1].action = RebaseAction::Reword;
    let evil_message =
        format!("reword; rm -rf {marker_display}; touch {marker_display}-via-reword #");
    plan.entries[1].message_override = Some(evil_message.clone());

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    assert!(matches!(result, RebaseResult::Completed { .. }));

    assert!(
        !marker.exists(),
        "the malicious subject must never have been executed as a shell command"
    );
    assert!(
        !repo_dir.path().join("PWNED_MARKER-via-reword").exists(),
        "the malicious reword message must never have been executed as a shell command"
    );

    // The malicious text landed exactly where it was supposed to: as
    // literal, unexecuted commit metadata.
    assert_eq!(commit_subject(repo_dir.path(), "HEAD~1"), evil_subject);
    assert_eq!(commit_subject(repo_dir.path(), "HEAD"), evil_message);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

/// A plan carrying entries built by hand (not read via `plan_rebase`) that
/// puts an invalid message on a `Pick` still gets caught by `validate`
/// before any mutation, mirroring the position-based rejection test.
#[test]
fn execute_rebase_plan_rejects_a_message_override_on_a_non_reword_action() {
    let repo_dir = init_repo("execute-rejects-message-on-pick");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "a.txt", "a\n");
    commit_all(repo_dir.path(), "only-commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut plan = provider.plan_rebase(&repo, base.as_str()).unwrap();
    plan.entries[0].message_override = Some("should not be allowed on a pick".to_string());

    let err = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

#[test]
fn execute_rebase_plan_with_no_entries_behaves_like_a_plain_rebase_no_op() {
    let repo_dir = init_repo("execute-empty-plan");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base");
    let head = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    // `onto` == `HEAD`: nothing to reapply.
    let plan = provider.plan_rebase(&repo, "HEAD").unwrap();
    assert!(plan.entries.is_empty());

    let result = RepositoryWritePort::execute_rebase_plan(&provider, &repo, &plan).unwrap();
    match result {
        RebaseResult::Completed { new_head } => assert_eq!(new_head, head),
        other => panic!("expected Completed, got {other:?}"),
    }
}
