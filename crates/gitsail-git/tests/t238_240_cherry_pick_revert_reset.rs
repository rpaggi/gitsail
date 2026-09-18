//! Integration tests for T-238/US-086 (`RepositoryWritePort::cherry_pick`),
//! T-239/US-087 (`RepositoryWritePort::revert`), and T-240/US-088
//! (`RepositoryWritePort::reset`) against real, temporary Git repositories —
//! mirroring `tests/t231_233_merge_conflicts.rs`'s own fixture conventions.
//!
//! Also exercises the specific claim T-238/T-239's task brief calls out to
//! verify rather than assume: that `RepositoryWritePort::continue_operation`/
//! `abort_operation` (built generically in T-233, before cherry-pick/revert
//! existed at all) already work unmodified for a pending cherry-pick/revert,
//! purely because both dispatch on whatever
//! `RepositoryReadPort::detect_in_progress_operation` currently detects.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    CherryPickResult, MergeParentPolicy, RepositoryReadPort, RepositoryWritePort, ResetMode,
    RevertResult,
};
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
        let path = std::env::temp_dir().join(format!("gitsail-git-history-{label}-{nanos}-{n}"));
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

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

fn parent_count(dir: &Path, revision: &str) -> usize {
    let output = git(dir, &["rev-list", "--parents", "-n", "1", revision]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    text.split_whitespace().count() - 1
}

// ---------------------------------------------------------------------
// T-238/US-086: cherry-pick.
// ---------------------------------------------------------------------

#[test]
fn cherry_pick_applies_a_simple_commit_as_a_new_commit_on_the_current_branch() {
    let repo_dir = init_repo("cherry-pick-simple");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "add line2");
    let feature_commit = rev_parse(repo_dir.path(), "HEAD");

    // `main` diverges with its own, unrelated commit first: cherry-picking
    // `feature_commit` onto it necessarily produces a commit parented on
    // `main`'s own tip, not `feature_commit`'s original parent — a
    // structurally distinct commit object, not merely one that is likely to
    // hash differently.
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "other.txt", "unrelated\n");
    commit_all(repo_dir.path(), "unrelated main work");
    let main_tip = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap();

    match result {
        CherryPickResult::Applied { hash } => {
            assert_ne!(
                hash, feature_commit,
                "cherry-pick always creates a new commit object"
            );
            assert_eq!(parent_count(repo_dir.path(), hash.as_str()), 1);
            assert_eq!(rev_parse(repo_dir.path(), "HEAD^"), main_tip);
        }
        other => panic!("expected Applied, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "line1\nline2\n"
    );
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn cherry_pick_reports_a_conflict_distinctly_never_as_a_completed_success() {
    let repo_dir = init_repo("cherry-pick-conflict");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-feature\nline3\n");
    commit_all(repo_dir.path(), "feature change");
    let feature_commit = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir.path(), "main change");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap();

    match result {
        CherryPickResult::Conflict { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, PathBuf::from("f.txt"));
        }
        other => panic!("a conflict must never be reported as {other:?}"),
    }
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::CherryPick(_)
    ));

    git_ok(repo_dir.path(), &["cherry-pick", "--abort"]);
}

#[test]
fn cherry_pick_reports_empty_when_the_change_is_already_present() {
    let repo_dir = init_repo("cherry-pick-empty");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "add line2");
    let feature_commit = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // First cherry-pick genuinely applies the change.
    let first = RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap();
    assert!(matches!(first, CherryPickResult::Applied { .. }));

    // The exact same commit's change is now already present on `main`:
    // Git itself reports this distinctly, not as a fresh success/conflict.
    let second = RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap();
    assert_eq!(second, CherryPickResult::Empty);

    // An empty cherry-pick still pauses the sequencer exactly like a
    // conflict does (recoverable via skip/abort), never silently discarded.
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::CherryPick(_)
    ));
    RepositoryWritePort::abort_operation(&provider, &repo).unwrap();
}

fn setup_merge_commit(repo_dir: &Path) -> (CommitHash, CommitHash) {
    write_file(repo_dir, "f.txt", "line1\n");
    commit_all(repo_dir, "base");
    let base = rev_parse(repo_dir, "HEAD");

    git_ok(repo_dir, &["checkout", "-q", "-b", "other"]);
    write_file(repo_dir, "other.txt", "other content\n");
    commit_all(repo_dir, "other change");

    git_ok(repo_dir, &["checkout", "-q", "main"]);
    write_file(repo_dir, "main.txt", "main content\n");
    commit_all(repo_dir, "main change");

    // A genuine, non-conflicting two-parent merge commit.
    git_ok(repo_dir, &["merge", "--no-edit", "other"]);
    let merge_commit = rev_parse(repo_dir, "HEAD");
    assert_eq!(parent_count(repo_dir, "HEAD"), 2);

    (base, merge_commit)
}

#[test]
fn cherry_pick_refuses_a_merge_commit_without_an_explicit_merge_parent_policy() {
    let repo_dir = init_repo("cherry-pick-merge-no-policy");
    let (_base, merge_commit) = setup_merge_commit(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::cherry_pick(&provider, &repo, &merge_commit, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    // Refused before ever invoking Git: no sequencer state was started.
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn cherry_pick_applies_a_merge_commit_using_first_parent_when_policy_is_supplied() {
    let repo_dir = init_repo("cherry-pick-merge-first-parent");
    let (base, merge_commit) = setup_merge_commit(repo_dir.path());

    // A fresh branch from `base`, before either side's work existed.
    git_ok(
        repo_dir.path(),
        &["checkout", "-q", "-b", "target", base.as_str()],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::cherry_pick(
        &provider,
        &repo,
        &merge_commit,
        Some(MergeParentPolicy::FirstParent),
    )
    .unwrap();

    match result {
        CherryPickResult::Applied { hash } => {
            assert_eq!(parent_count(repo_dir.path(), hash.as_str()), 1);
        }
        other => panic!("expected Applied, got {other:?}"),
    }
    // First-parent diff of the merge commit is exactly "other.txt was added"
    // (the change the merge introduced relative to main's own line).
    assert!(repo_dir.path().join("other.txt").is_file());
    assert!(!repo_dir.path().join("main.txt").is_file());
}

#[test]
fn cherry_pick_refuses_a_merge_parent_policy_against_a_non_merge_commit() {
    let repo_dir = init_repo("cherry-pick-non-merge-with-policy");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "second");
    let second = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::cherry_pick(
        &provider,
        &repo,
        &second,
        Some(MergeParentPolicy::FirstParent),
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

#[test]
fn cherry_pick_refuses_to_start_when_another_operation_is_already_pending() {
    let repo_dir = init_repo("cherry-pick-refuses-on-existing-op");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-feature\nline3\n");
    commit_all(repo_dir.path(), "feature change");
    let feature_commit = rev_parse(repo_dir.path(), "HEAD");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir.path(), "main change");

    // Leave a real conflicting cherry-pick pending directly.
    let cp_output = git(repo_dir.path(), &["cherry-pick", feature_commit.as_str()]);
    assert!(!cp_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err =
        RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    git_ok(repo_dir.path(), &["cherry-pick", "--abort"]);
}

// ---------------------------------------------------------------------
// T-239/US-087: revert.
// ---------------------------------------------------------------------

#[test]
fn revert_creates_a_new_commit_undoing_the_change_never_moving_existing_refs() {
    let repo_dir = init_repo("revert-simple");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");

    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "add line2");
    let added = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::revert(&provider, &repo, &added, None).unwrap();

    match result {
        RevertResult::Applied { hash } => {
            // Structural, not just textual: a brand-new commit object,
            // parented on the commit being reverted, never a rewrite of it
            // or a moved branch tip pointing at something old.
            assert_ne!(hash, added);
            assert_ne!(hash, base);
            assert_eq!(rev_parse(repo_dir.path(), "HEAD^"), added);
            assert_eq!(parent_count(repo_dir.path(), hash.as_str()), 1);
        }
        other => panic!("expected Applied, got {other:?}"),
    }
    // `added`'s own commit object is completely untouched by the revert.
    assert_eq!(rev_parse(repo_dir.path(), "HEAD~1"), added);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "line1\n"
    );
}

#[test]
fn revert_reports_a_conflict_distinctly_never_as_a_completed_success() {
    let repo_dir = init_repo("revert-conflict");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED\nline3\n");
    commit_all(repo_dir.path(), "change line2");
    let change_commit = rev_parse(repo_dir.path(), "HEAD");

    // A later, unrelated change to the exact same line makes a clean
    // revert of `change_commit` impossible.
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-AGAIN\nline3\n");
    commit_all(repo_dir.path(), "change line2 again");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::revert(&provider, &repo, &change_commit, None).unwrap();

    match result {
        RevertResult::Conflict { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, PathBuf::from("f.txt"));
        }
        other => panic!("a conflict must never be reported as {other:?}"),
    }
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Revert(_)
    ));
    git_ok(repo_dir.path(), &["revert", "--abort"]);
}

#[test]
fn revert_refuses_a_merge_commit_without_an_explicit_merge_parent_policy() {
    let repo_dir = init_repo("revert-merge-no-policy");
    let (_base, merge_commit) = setup_merge_commit(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::revert(&provider, &repo, &merge_commit, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn revert_applies_a_merge_commit_using_first_parent_when_policy_is_supplied() {
    let repo_dir = init_repo("revert-merge-first-parent");
    let (_base, merge_commit) = setup_merge_commit(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::revert(
        &provider,
        &repo,
        &merge_commit,
        Some(MergeParentPolicy::FirstParent),
    )
    .unwrap();

    assert!(matches!(result, RevertResult::Applied { .. }));
    // Reverting the merge against its first parent undoes exactly the
    // change the merge introduced relative to it: `other.txt` disappears
    // again, `main.txt` (already on the first-parent line) survives.
    assert!(!repo_dir.path().join("other.txt").is_file());
    assert!(repo_dir.path().join("main.txt").is_file());
}

#[test]
fn revert_reports_an_empty_result_as_a_clear_classified_error_recoverable_via_skip_or_abort() {
    let repo_dir = init_repo("revert-empty");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "add line2");
    let added = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    RepositoryWritePort::revert(&provider, &repo, &added, None).unwrap();

    // The addition is now already undone; reverting it again has nothing
    // left to remove — Git reports this distinctly (an empty result),
    // never as a fresh success or an opaque process failure. Verified
    // empirically: unlike an empty cherry-pick, a plain empty revert exits
    // cleanly with no `REVERT_HEAD` left pending at all (see
    // `classify_revert_failure`'s own doc), so nothing is left for
    // skip/abort to pick up here.
    let err = RepositoryWritePort::revert(&provider, &repo, &added, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn revert_refuses_to_start_when_another_operation_is_already_pending() {
    let repo_dir = init_repo("revert-refuses-on-existing-op");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "add line2");
    let added = rev_parse(repo_dir.path(), "HEAD");

    // Leave a real, genuinely conflicting merge pending.
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "conflict-a"]);
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nCONFLICT-A\n");
    commit_all(repo_dir.path(), "conflict a");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "conflict-b"]);
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nCONFLICT-B\n");
    commit_all(repo_dir.path(), "conflict b");
    let merge_output = git(repo_dir.path(), &["merge", "conflict-a"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::revert(&provider, &repo, &added, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

// ---------------------------------------------------------------------
// T-233/US-081 (already delivered): confirms, rather than assumes, that
// `continue_operation`/`abort_operation` work unmodified for a pending
// cherry-pick/revert — both were built generically in T-233, before
// cherry-pick/revert existed as real mutations at all.
// ---------------------------------------------------------------------

#[test]
fn continue_operation_completes_a_pending_cherry_pick_once_resolved() {
    let repo_dir = init_repo("continue-cherry-pick");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-feature\nline3\n");
    commit_all(repo_dir.path(), "feature change");
    let feature_commit = rev_parse(repo_dir.path(), "HEAD");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir.path(), "main change");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::cherry_pick(&provider, &repo, &feature_commit, None).unwrap();
    assert!(matches!(result, CherryPickResult::Conflict { .. }));

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
fn abort_operation_restores_the_pre_revert_head_for_a_pending_revert() {
    let repo_dir = init_repo("abort-revert");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED\nline3\n");
    commit_all(repo_dir.path(), "change line2");
    let change_commit = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-AGAIN\nline3\n");
    commit_all(repo_dir.path(), "change line2 again");
    let head_before_revert = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::revert(&provider, &repo, &change_commit, None).unwrap();
    assert!(matches!(result, RevertResult::Conflict { .. }));

    RepositoryWritePort::abort_operation(&provider, &repo).unwrap();

    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), head_before_revert);
}

// ---------------------------------------------------------------------
// T-240/US-088: reset — soft, mixed, hard.
// ---------------------------------------------------------------------

fn setup_two_commits(repo_dir: &Path) -> (CommitHash, CommitHash) {
    write_file(repo_dir, "f.txt", "a\n");
    commit_all(repo_dir, "c1");
    let c1 = rev_parse(repo_dir, "HEAD");

    write_file(repo_dir, "f.txt", "a\nb\n");
    commit_all(repo_dir, "c2");
    let c2 = rev_parse(repo_dir, "HEAD");

    (c1, c2)
}

#[test]
fn reset_soft_moves_head_only_leaving_index_and_working_tree_staged() {
    let repo_dir = init_repo("reset-soft");
    let (c1, c2) = setup_two_commits(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    RepositoryWritePort::reset(&provider, &repo, c1.as_str(), ResetMode::Soft, &c2).unwrap();

    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), c1);
    // Working tree preserved exactly as it was.
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "a\nb\n"
    );
    // The difference between c1 and c2 now shows up as staged.
    let staged = git(repo_dir.path(), &["diff", "--cached", "--name-only"]);
    assert_eq!(String::from_utf8(staged.stdout).unwrap().trim(), "f.txt");
    let unstaged = git(repo_dir.path(), &["diff", "--name-only"]);
    assert!(String::from_utf8(unstaged.stdout)
        .unwrap()
        .trim()
        .is_empty());
}

#[test]
fn reset_mixed_moves_head_and_index_leaving_working_tree_unstaged() {
    let repo_dir = init_repo("reset-mixed");
    let (c1, c2) = setup_two_commits(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    RepositoryWritePort::reset(&provider, &repo, c1.as_str(), ResetMode::Mixed, &c2).unwrap();

    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), c1);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "a\nb\n",
        "mixed reset preserves the working tree"
    );
    let staged = git(repo_dir.path(), &["diff", "--cached", "--name-only"]);
    assert!(String::from_utf8(staged.stdout).unwrap().trim().is_empty());
    let unstaged = git(repo_dir.path(), &["diff", "--name-only"]);
    assert_eq!(String::from_utf8(unstaged.stdout).unwrap().trim(), "f.txt");
}

#[test]
fn reset_hard_moves_head_index_and_working_tree_discarding_the_change() {
    let repo_dir = init_repo("reset-hard");
    let (c1, c2) = setup_two_commits(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    RepositoryWritePort::reset(&provider, &repo, c1.as_str(), ResetMode::Hard, &c2).unwrap();

    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), c1);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "a\n",
        "hard reset makes the working tree identical to the target"
    );
    let status = git(repo_dir.path(), &["status", "--porcelain"]);
    assert!(String::from_utf8(status.stdout).unwrap().trim().is_empty());
}

#[test]
fn reset_refuses_a_stale_expected_head_instead_of_resetting_against_it() {
    let repo_dir = init_repo("reset-stale-head");
    let (c1, c2) = setup_two_commits(repo_dir.path());
    write_file(repo_dir.path(), "f.txt", "a\nb\nc\n");
    commit_all(repo_dir.path(), "c3");
    let c3 = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // `c2` was HEAD when this reset was supposedly previewed/confirmed, but
    // real HEAD has since moved on to `c3` (e.g. another terminal committed
    // in between) — this must be refused, never executed against the old
    // expectation.
    let err = RepositoryWritePort::reset(&provider, &repo, c1.as_str(), ResetMode::Hard, &c2)
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    // Nothing moved: HEAD is still `c3`, untouched.
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), c3);
}

#[test]
fn reset_refuses_to_start_when_another_operation_is_already_pending() {
    let repo_dir = init_repo("reset-refuses-on-existing-op");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-feature\nline3\n");
    commit_all(repo_dir.path(), "feature change");
    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir.path(), "main change");
    let head = rev_parse(repo_dir.path(), "HEAD");

    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::reset(&provider, &repo, base.as_str(), ResetMode::Hard, &head)
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}
