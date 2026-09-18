//! Integration tests for EPIC-19 (Remote Operations; Takumi E-42):
//! T-211/US-096 (fetch), T-212/US-097 (fast-forward-only pull), T-213/
//! US-098 (common push), T-214/US-099 (protected force push with lease),
//! and T-215/US-100 (transport recovery/diagnostic consolidation).
//!
//! Mirrors `tests/epic18_stash_tags_worktrees.rs`'s own convention: every
//! test creates real, temporary Git repositories via the `git` CLI (never a
//! mock), never touches a real network — every "remote" here is another
//! local, on-disk repository (bare, so it can be pushed to directly) — and
//! exercises [`GitCliProvider`] through `RepositoryReadPort`/
//! `RepositoryWritePort`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{Precondition, PullOutcome, RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::{BranchKind, BranchName, CancellationToken, CommitHash, ErrorCode};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

// ---------------------------------------------------------------------
// Fixture plumbing (mirrors `tests/epic18_stash_tags_worktrees.rs`'s own
// helpers).
// ---------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-epic19-{label}-{nanos}-{n}"));
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

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

/// A bare repository standing in for a real remote (US-096's DoD: "um
/// remote local de teste ... sem depender de rede real"). Bare because a
/// non-bare repository refuses a push to its checked-out branch by
/// default — using a bare one is what lets these tests push freely, the
/// same reason any real Git hosting service's repositories are bare.
fn init_bare_remote(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(
        dir.path(),
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    dir
}

fn clone_repo(remote: &Path, label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(
        dir.path().parent().unwrap(),
        &[
            "clone",
            "--quiet",
            "--",
            remote.to_str().unwrap(),
            dir.path().to_str().unwrap(),
        ],
    );
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", message]);
}

fn current_head(dir: &Path) -> String {
    git_output(dir, &["rev-parse", "HEAD"])
}

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

fn main_branch() -> BranchName {
    BranchName::new("main").unwrap()
}

// =======================================================================
// T-211/US-096: fetch.
// =======================================================================

#[test]
fn fetch_updates_remote_tracking_refs_without_touching_the_working_tree() {
    let remote_dir = init_bare_remote("fetch-remote");
    let local_dir = clone_repo(remote_dir.path(), "fetch-local");
    write_file(local_dir.path(), "a.txt", "one\n");
    commit_all(local_dir.path(), "first commit");
    git(
        local_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    // Someone else pushes a new commit to the remote after this clone was
    // taken.
    let other_dir = clone_repo(remote_dir.path(), "fetch-other");
    write_file(other_dir.path(), "b.txt", "from elsewhere\n");
    commit_all(other_dir.path(), "commit from elsewhere");
    git(
        other_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let remote_head_after = current_head(other_dir.path());

    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();
    let local_head_before = current_head(local_dir.path());

    provider
        .fetch(&repo, "origin", &CancellationToken::new())
        .expect("fetching a reachable local remote must succeed");

    let branches = provider.branches(&repo).unwrap();
    let remote_tracking = branches
        .iter()
        .find(|b| {
            b.kind
                == BranchKind::Remote {
                    remote: "origin".to_string(),
                }
                && b.name.as_str() == "main"
        })
        .expect("fetch must create/update refs/remotes/origin/main");
    assert_eq!(remote_tracking.target.as_str(), remote_head_after);

    // The working tree/index/HEAD must be completely untouched by fetch.
    assert_eq!(current_head(local_dir.path()), local_head_before);
    assert!(!local_dir.path().join("b.txt").exists());
    let status = provider.status(&repo).unwrap();
    assert!(
        status.is_clean(),
        "fetch must never modify the working tree"
    );
}

#[test]
fn fetch_from_a_nonexistent_local_remote_reports_a_clear_network_failure() {
    let repo_dir = init_repo("fetch-missing-remote");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let missing_path = repo_dir.path().join("does-not-exist-anywhere");
    let err = provider
        .fetch(
            &repo,
            missing_path.to_str().unwrap(),
            &CancellationToken::new(),
        )
        .expect_err("fetching an inaccessible remote must fail, never silently succeed");

    assert_eq!(err.code(), ErrorCode::NetworkFailure);
}

// =======================================================================
// T-212/US-097: pull (fast-forward only).
// =======================================================================

#[test]
fn pull_fast_forwards_a_behind_branch_and_reports_the_new_head() {
    let remote_dir = init_bare_remote("pull-ff-remote");
    let behind_dir = clone_repo(remote_dir.path(), "pull-ff-behind");
    write_file(behind_dir.path(), "a.txt", "one\n");
    commit_all(behind_dir.path(), "first commit");
    git(
        behind_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    let ahead_dir = clone_repo(remote_dir.path(), "pull-ff-ahead");
    write_file(ahead_dir.path(), "b.txt", "two\n");
    commit_all(ahead_dir.path(), "second commit");
    git(
        ahead_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let new_remote_head = current_head(ahead_dir.path());

    let provider = provider();
    let repo = provider.discover(behind_dir.path()).unwrap();

    let outcome = provider
        .pull(&repo, "origin", &main_branch(), &CancellationToken::new())
        .expect("a plain fast-forward pull must succeed");

    match outcome {
        PullOutcome::FastForwarded { new_head } => {
            assert_eq!(new_head.as_str(), new_remote_head);
        }
        PullOutcome::AlreadyUpToDate => panic!("expected a fast-forward, not a no-op"),
    }
    assert_eq!(current_head(behind_dir.path()), new_remote_head);
    assert!(behind_dir.path().join("b.txt").exists());
}

#[test]
fn pull_reports_already_up_to_date_when_there_is_nothing_to_integrate() {
    let remote_dir = init_bare_remote("pull-uptodate-remote");
    let local_dir = clone_repo(remote_dir.path(), "pull-uptodate-local");
    write_file(local_dir.path(), "a.txt", "one\n");
    commit_all(local_dir.path(), "first commit");
    git(
        local_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();

    let outcome = provider
        .pull(&repo, "origin", &main_branch(), &CancellationToken::new())
        .unwrap();

    assert_eq!(outcome, PullOutcome::AlreadyUpToDate);
}

#[test]
fn pull_refuses_diverged_branches_without_any_side_effect() {
    let remote_dir = init_bare_remote("pull-diverge-remote");
    let base_dir = clone_repo(remote_dir.path(), "pull-diverge-base");
    write_file(base_dir.path(), "a.txt", "one\n");
    commit_all(base_dir.path(), "first commit");
    git(
        base_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    // The remote advances with a commit the local clone never had...
    let remote_advances_dir = clone_repo(remote_dir.path(), "pull-diverge-remote-advance");
    write_file(remote_advances_dir.path(), "remote.txt", "remote change\n");
    commit_all(remote_advances_dir.path(), "remote-only commit");
    git(
        remote_advances_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    // ...while the local clone also advances independently, diverging.
    write_file(base_dir.path(), "local.txt", "local change\n");
    commit_all(base_dir.path(), "local-only commit");
    let local_head_before = current_head(base_dir.path());

    let provider = provider();
    let repo = provider.discover(base_dir.path()).unwrap();

    let err = provider
        .pull(&repo, "origin", &main_branch(), &CancellationToken::new())
        .expect_err("diverged branches must never be merged/rebased automatically");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    // Nothing about the local branch or working tree changed: no partial
    // merge, no reset, no discarded work.
    assert_eq!(current_head(base_dir.path()), local_head_before);
    assert!(!base_dir.path().join(".git/MERGE_HEAD").exists());
    let status = provider.status(&repo).unwrap();
    assert!(
        status.is_clean(),
        "a refused fast-forward-only pull must leave the working tree exactly as it was"
    );
}

// =======================================================================
// T-213/US-098: push.
// =======================================================================

#[test]
fn push_publishes_local_commits_to_a_common_bare_remote() {
    let remote_dir = init_bare_remote("push-common-remote");
    let local_dir = clone_repo(remote_dir.path(), "push-common-local");
    write_file(local_dir.path(), "a.txt", "one\n");
    commit_all(local_dir.path(), "first commit");
    let local_head = current_head(local_dir.path());

    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();

    provider
        .push(&repo, "origin", &main_branch(), &CancellationToken::new())
        .expect("pushing new commits to an empty bare remote must succeed");

    let remote_head = git_output(remote_dir.path(), &["rev-parse", "refs/heads/main"]);
    assert_eq!(remote_head, local_head);
}

#[test]
fn push_rejects_a_non_fast_forward_and_preserves_the_remotes_state() {
    let remote_dir = init_bare_remote("push-reject-remote");
    let seed_dir = clone_repo(remote_dir.path(), "push-reject-seed");
    write_file(seed_dir.path(), "a.txt", "one\n");
    commit_all(seed_dir.path(), "first commit");
    git(
        seed_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    // Another clone pushes first, advancing the remote.
    let other_dir = clone_repo(remote_dir.path(), "push-reject-other");
    write_file(other_dir.path(), "b.txt", "from elsewhere\n");
    commit_all(other_dir.path(), "commit from elsewhere");
    git(
        other_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let remote_head_after_other_push = current_head(other_dir.path());

    // This clone, unaware of that push, commits on top of the old base and
    // tries to push — a non-fast-forward rejection.
    write_file(seed_dir.path(), "c.txt", "unaware local commit\n");
    commit_all(seed_dir.path(), "local commit unaware of remote advance");

    let provider = provider();
    let repo = provider.discover(seed_dir.path()).unwrap();

    let err = provider
        .push(&repo, "origin", &main_branch(), &CancellationToken::new())
        .expect_err("a non-fast-forward push must never succeed, and never auto-force");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    let remote_head_after_rejected_push =
        git_output(remote_dir.path(), &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        remote_head_after_rejected_push, remote_head_after_other_push,
        "the remote's real state (someone else's work) must be untouched by the rejected push"
    );
}

#[test]
fn push_to_an_inaccessible_remote_reports_network_failure_without_a_false_success() {
    let repo_dir = init_repo("push-missing-remote");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let missing_path = repo_dir.path().join("does-not-exist-anywhere");

    let err = provider
        .push(
            &repo,
            missing_path.to_str().unwrap(),
            &main_branch(),
            &CancellationToken::new(),
        )
        .expect_err("pushing to an inaccessible remote must fail, never silently succeed");

    assert_eq!(err.code(), ErrorCode::NetworkFailure);
}

// =======================================================================
// T-214/US-099: force push with lease.
// =======================================================================

#[test]
fn force_push_with_lease_is_accepted_when_the_remote_still_matches_the_expected_head() {
    let remote_dir = init_bare_remote("force-lease-ok-remote");
    let local_dir = clone_repo(remote_dir.path(), "force-lease-ok-local");
    write_file(local_dir.path(), "a.txt", "one\n");
    commit_all(local_dir.path(), "first commit");
    git(
        local_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let expected_remote_head = CommitHash::new(current_head(local_dir.path())).unwrap();

    // Rewrite local history (amend), the classic force-push scenario.
    write_file(local_dir.path(), "a.txt", "one, amended\n");
    git(local_dir.path(), &["add", "-A"]);
    git(
        local_dir.path(),
        &[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "first commit (amended)",
        ],
    );
    let rewritten_head = current_head(local_dir.path());

    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();

    provider
        .force_push_with_lease(
            &repo,
            "origin",
            &main_branch(),
            &Precondition::new(expected_remote_head),
            &CancellationToken::new(),
        )
        .expect("the lease matches the remote's real tip, so this must be accepted");

    let remote_head = git_output(remote_dir.path(), &["rev-parse", "refs/heads/main"]);
    assert_eq!(remote_head, rewritten_head);
}

/// DoD: "teste com um avanço concorrente do remote simulado (outro
/// processo/checkout empurra algo entre a leitura do estado esperado e o
/// force push) comprova rejeição pelo lease e preservação do trabalho
/// remoto alheio."
#[test]
fn force_push_with_lease_is_rejected_by_a_concurrent_remote_advance_and_preserves_it() {
    let remote_dir = init_bare_remote("force-lease-race-remote");
    let local_dir = clone_repo(remote_dir.path(), "force-lease-race-local");
    write_file(local_dir.path(), "a.txt", "one\n");
    commit_all(local_dir.path(), "first commit");
    git(
        local_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    // The caller observes the remote's current tip here — this is the value
    // it will build its lease around.
    let expected_remote_head = CommitHash::new(current_head(local_dir.path())).unwrap();

    // Between that observation and the force push below, a *different*
    // checkout of the same remote pushes something else — simulating a
    // concurrent collaborator (or another GitSail session) advancing the
    // remote in the meantime.
    let concurrent_dir = clone_repo(remote_dir.path(), "force-lease-race-concurrent");
    write_file(
        concurrent_dir.path(),
        "concurrent.txt",
        "someone else's work\n",
    );
    commit_all(concurrent_dir.path(), "a concurrent, unrelated commit");
    git(
        concurrent_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let remote_head_after_concurrent_push = current_head(concurrent_dir.path());

    // The original caller, still only knowing the now-stale
    // `expected_remote_head`, rewrites its own local history and attempts
    // the force push it originally intended.
    write_file(local_dir.path(), "a.txt", "one, rewritten\n");
    git(local_dir.path(), &["add", "-A"]);
    git(
        local_dir.path(),
        &[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "rewritten independently",
        ],
    );

    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();

    let err = provider
        .force_push_with_lease(
            &repo,
            "origin",
            &main_branch(),
            &Precondition::new(expected_remote_head),
            &CancellationToken::new(),
        )
        .expect_err(
            "the remote no longer matches the captured lease, so this must be refused, never silently forced",
        );

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    let remote_head_after_rejected_force_push =
        git_output(remote_dir.path(), &["rev-parse", "refs/heads/main"]);
    assert_eq!(
        remote_head_after_rejected_force_push, remote_head_after_concurrent_push,
        "the concurrent collaborator's work must survive the refused force push untouched"
    );
}

// =======================================================================
// T-215/US-100: transport recovery/diagnostic consolidation.
// =======================================================================

/// A retry must reconsult real state rather than blindly repeating the
/// previous (failed) attempt: fixing the actual problem (a misconfigured
/// remote name) and retrying the *same* call must succeed and reflect the
/// real, current world — not replay the earlier failure.
#[test]
fn retrying_fetch_after_correcting_a_bad_remote_reconsults_real_state() {
    let remote_dir = init_bare_remote("retry-fetch-remote");
    let seed_dir = clone_repo(remote_dir.path(), "retry-fetch-seed");
    write_file(seed_dir.path(), "a.txt", "one\n");
    commit_all(seed_dir.path(), "first commit");
    git(
        seed_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );

    let local_dir = clone_repo(remote_dir.path(), "retry-fetch-local");
    let provider = provider();
    let repo = provider.discover(local_dir.path()).unwrap();

    let missing_path = local_dir.path().join("does-not-exist-anywhere");
    let first_attempt = provider.fetch(
        &repo,
        missing_path.to_str().unwrap(),
        &CancellationToken::new(),
    );
    assert_eq!(first_attempt.unwrap_err().code(), ErrorCode::NetworkFailure);

    // The remote advances in the meantime (this is real, current state a
    // blind repeat of the first attempt could never have picked up).
    write_file(seed_dir.path(), "b.txt", "two\n");
    commit_all(seed_dir.path(), "second commit");
    git(
        seed_dir.path(),
        &["push", "--quiet", "--", "origin", "main"],
    );
    let latest_remote_head = current_head(seed_dir.path());

    // Retrying against the real remote (not the broken path) must succeed
    // and reflect the *current* remote tip, not anything cached from the
    // failed attempt.
    provider
        .fetch(&repo, "origin", &CancellationToken::new())
        .expect("a retry against the real remote must succeed");

    let branches = provider.branches(&repo).unwrap();
    let remote_tracking = branches
        .iter()
        .find(|b| {
            b.kind
                == BranchKind::Remote {
                    remote: "origin".to_string(),
                }
                && b.name.as_str() == "main"
        })
        .unwrap();
    assert_eq!(remote_tracking.target.as_str(), latest_remote_head);
}
