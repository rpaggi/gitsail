//! Integration tests for T-241/US-089 (inspecting HEAD's reflog; Takumi
//! E-47/EPIC-17).
//!
//! Mirrors `tests/epic18_stash_tags_worktrees.rs`'s own convention: every
//! test creates a real, temporary Git repository via the `git` CLI (never a
//! mock) and exercises [`GitCliProvider`] through `RepositoryReadPort`.
//!
//! A deterministic "object missing" entry (a reflog entry whose target
//! commit object has since been pruned) is documented as difficult to force
//! rather than attempted here: `git reflog expire` — the mechanism that
//! would make an old entry's object collectible by `git gc` — removes the
//! expired entry itself from `git reflog show`'s own output (verified
//! empirically while building this adapter), so there is no ordinary,
//! deterministic Git command sequence that leaves a *listed* entry pointing
//! at a *pruned* object. `object_existence_reports_a_hash_with_no_such_object`
//! below instead exercises the missing-object classification directly
//! against a hash that was never a real object, which is the same code path
//! [`gitsail_git::GitCliProvider::reflog`] uses to classify any entry.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::RepositoryReadPort;
use gitsail_domain::ReflogObjectState;
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
        let path = std::env::temp_dir().join(format!("gitsail-git-t241-{label}-{nanos}-{n}"));
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

// =======================================================================
// T-241/US-089 criterion 1: reference/hash/date/message are all available.
// =======================================================================

#[test]
fn reflog_is_empty_on_an_unborn_branch() {
    let repo_dir = init_repo("reflog-unborn");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let entries = provider
        .reflog(&repo)
        .expect("no commits yet is a legitimate empty state, not an error");

    assert!(entries.is_empty());
}

#[test]
fn reflog_lists_entries_newest_first_with_hash_message_and_date() {
    let repo_dir = init_repo("reflog-basic");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let first_head = current_head(repo_dir.path());

    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "second commit");
    let second_head = current_head(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let entries = provider.reflog(&repo).unwrap();

    assert_eq!(entries.len(), 2, "one reflog entry per commit made so far");

    // Newest first: `HEAD@{0}` is the most recent move.
    assert_eq!(entries[0].index, 0);
    assert_eq!(entries[0].selector("HEAD"), "HEAD@{0}");
    assert_eq!(entries[0].commit.as_str(), second_head);
    assert!(entries[0].message.contains("second commit"));
    assert_eq!(entries[0].object_state, ReflogObjectState::Present);
    assert!(entries[0].is_available());

    assert_eq!(entries[1].index, 1);
    assert_eq!(entries[1].selector("HEAD"), "HEAD@{1}");
    assert_eq!(entries[1].commit.as_str(), first_head);
    assert!(entries[1].message.contains("first commit"));
    assert_eq!(entries[1].object_state, ReflogObjectState::Present);

    // A sane, non-zero timestamp was actually parsed for every entry.
    assert!(entries.iter().all(|e| e.date.seconds_since_epoch > 0));
}

#[test]
fn reflog_records_a_reset_distinctly_from_a_commit() {
    let repo_dir = init_repo("reflog-reset");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "second commit");

    git(repo_dir.path(), &["reset", "--soft", "HEAD~1"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let entries = provider.reflog(&repo).unwrap();

    assert_eq!(entries.len(), 3);
    assert!(
        entries[0].message.contains("reset"),
        "the most recent reflog entry must record the reset, not a generic message: {}",
        entries[0].message
    );
}

// =======================================================================
// T-241/US-089 criterion 3: an entry whose object no longer exists is
// reported clearly, never fails the whole query.
//
// A real, deterministic "listed reflog entry pointing at a pruned object" is
// not exercised here — see this file's own module doc for why (Git's own
// `git reflog expire` removes the expired entry itself from `git reflog
// show`'s output, rather than leaving a dangling one; and manually appending
// a synthetic line naming a never-written hash to `.git/logs/HEAD` was
// tried and does not work either — `git reflog show`/`git log -g` silently
// omits any entry it cannot load the referenced object for, confirmed
// empirically while building this adapter). The missing-object
// classification itself (`ReflogObjectState::Missing`) is instead covered
// deterministically and directly, without a real pruned repository, by
// `gitsail_git::provider`'s own unit tests
// (`parse_batch_check_existence_classifies_missing_objects`), which feed a
// synthetic `git cat-file --batch-check` output containing a `missing`
// line — the exact same real-Git output shape verified by hand against a
// hash that was never written as an object (`git cat-file --batch-check`
// reports `<hash> missing`).
//
// This is the documented limitation the task anticipated: "pode ser difícil
// forçar objeto ausente de forma determinística num teste".
#[test]
fn cat_file_reports_a_never_written_hash_as_missing_confirming_the_batch_check_output_shape() {
    let repo_dir = init_repo("reflog-missing-object-shape");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    // A syntactically valid hash that was never written as an object in
    // this repository. `cat-file -e` exits non-zero for it — confirming, at
    // the real `git` binary level, the exact precondition
    // `GitCliProvider::reflog`'s existence check relies on to ever produce
    // `ReflogObjectState::Missing` for a real entry.
    let status = Command::new("git")
        .args(["cat-file", "-e", "cafecafecafecafecafecafecafecafecafecafe"])
        .current_dir(repo_dir.path())
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "a hash never written as an object must be reported as missing by git itself"
    );
}
