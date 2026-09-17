//! Integration tests for [`GitCliProvider`] (SAD §31): every test creates a
//! real, temporary Git repository via the `git` CLI (never a mock) and
//! exercises `GitCliProvider` against it through `RepositoryReadPort`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{CommitQuery, DiffRequest, RepositoryReadPort};
use gitsail_domain::{BranchKind, ChangeType, CommitHash, Decoration, ErrorCode, HeadState};
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
        let path = std::env::temp_dir().join(format!("gitsail-git-provider-{label}-{nanos}-{n}"));
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

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

// ---------------------------------------------------------------------
// discover / status: empty (unborn) repository.
// ---------------------------------------------------------------------

#[test]
fn discovers_an_unborn_repository() {
    let repo_dir = init_repo("unborn-discover");
    let provider = provider();

    let repo = provider
        .discover(repo_dir.path())
        .expect("discover should succeed for a freshly initialized repository");

    assert!(!repo.is_bare);
    assert_eq!(repo.head_state, HeadState::Unborn);
    assert_eq!(repo.current_branch, None);
    assert_eq!(repo.worktree_path.as_deref(), Some(repo.root_path.as_path()));
}

#[test]
fn status_on_an_unborn_repository_reports_unborn_with_no_files() {
    let repo_dir = init_repo("unborn-status");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let status = provider.status(&repo).expect("status should succeed on an unborn repository");

    assert_eq!(status.head_state, HeadState::Unborn);
    assert!(status.branch.is_none());
    assert!(status.files.is_empty());
}

#[test]
fn commit_history_on_an_unborn_repository_is_an_empty_page() {
    let repo_dir = init_repo("unborn-log");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(&repo, &CommitQuery::default())
        .expect("commits should succeed (as an empty page) on an unborn repository");

    assert!(page.items.is_empty());
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, None);
}

// ---------------------------------------------------------------------
// One commit: discover, status, commits, commit, decorations.
// ---------------------------------------------------------------------

#[test]
fn discovers_a_repository_with_one_commit_on_a_branch() {
    let repo_dir = init_repo("one-commit-discover");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    match &repo.head_state {
        HeadState::Attached { branch } => assert_eq!(branch.as_str(), "main"),
        other => panic!("expected Attached, got {other:?}"),
    }
    assert_eq!(repo.current_branch.as_ref().map(|b| b.as_str()), Some("main"));
}

#[test]
fn fetches_commit_history_and_a_single_commit_by_hash() {
    let repo_dir = init_repo("one-commit-history");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit\n\nwith a body line");
    git(repo_dir.path(), &["tag", "v1.0"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(&repo, &CommitQuery::default())
        .expect("commits should succeed");
    assert_eq!(page.items.len(), 1);
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, None);

    let commit = &page.items[0];
    assert_eq!(commit.subject, "first commit");
    // `%b` preserves the raw body exactly as Git stores it, including its
    // trailing newline.
    assert_eq!(commit.body, "with a body line\n");
    assert_eq!(commit.author.name, "Test User");
    assert_eq!(commit.author.email, "test@example.com");
    assert!(commit.is_root());
    assert!(!commit.is_merge());
    assert!(commit
        .decorations
        .iter()
        .any(|d| matches!(d, Decoration::Head)));
    assert!(commit
        .decorations
        .iter()
        .any(|d| matches!(d, Decoration::Tag(tag) if tag == "v1.0")));

    let fetched = provider
        .commit(&repo, &commit.hash)
        .expect("fetching the same commit by hash should succeed");
    assert_eq!(fetched, *commit);
}

#[test]
fn fetching_an_unknown_commit_hash_reports_repository_not_found() {
    let repo_dir = init_repo("unknown-commit");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let missing = CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();

    let err = provider
        .commit(&repo, &missing)
        .expect_err("an unknown commit hash should fail");
    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
}

// ---------------------------------------------------------------------
// Multiple branches.
// ---------------------------------------------------------------------

#[test]
fn lists_multiple_local_branches_with_the_current_one_marked() {
    let repo_dir = init_repo("multi-branch");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["branch", "feature"]);
    git(repo_dir.path(), &["branch", "release"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let branches = provider.branches(&repo).expect("branches should succeed");
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature"));
    assert!(names.contains(&"release"));
    assert!(branches
        .iter()
        .all(|b| matches!(b.kind, BranchKind::Local)));

    let current: Vec<&str> = branches
        .iter()
        .filter(|b| b.is_current)
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(current, vec!["main"]);
}

// ---------------------------------------------------------------------
// Detached HEAD.
// ---------------------------------------------------------------------

#[test]
fn discovers_a_detached_head() {
    let repo_dir = init_repo("detached");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "hello again\n");
    commit_all(repo_dir.path(), "second commit");

    let head_output = Command::new("git")
        .args(["rev-parse", "HEAD~1"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    let target = String::from_utf8(head_output.stdout).unwrap().trim().to_string();
    git(repo_dir.path(), &["checkout", "--quiet", &target]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    match &repo.head_state {
        HeadState::Detached { commit } => assert_eq!(commit.as_str(), target),
        other => panic!("expected Detached, got {other:?}"),
    }
    assert_eq!(repo.current_branch, None);

    let status = provider.status(&repo).expect("status should succeed while detached");
    match &status.head_state {
        HeadState::Detached { commit } => assert_eq!(commit.as_str(), target),
        other => panic!("expected Detached in status, got {other:?}"),
    }
    assert!(status.branch.is_none());
}

// ---------------------------------------------------------------------
// Dirty tree: staged, unstaged and untracked changes together.
// ---------------------------------------------------------------------

#[test]
fn status_reports_staged_unstaged_and_untracked_changes() {
    let repo_dir = init_repo("dirty-tree");
    write_file(repo_dir.path(), "staged.txt", "original\n");
    write_file(repo_dir.path(), "unstaged.txt", "original\n");
    commit_all(repo_dir.path(), "first commit");

    // Staged: modify and `git add`.
    write_file(repo_dir.path(), "staged.txt", "staged change\n");
    git(repo_dir.path(), &["add", "staged.txt"]);
    // Unstaged: modify without staging.
    write_file(repo_dir.path(), "unstaged.txt", "unstaged change\n");
    // Untracked: a brand-new file.
    write_file(repo_dir.path(), "untracked.txt", "new\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let status = provider.status(&repo).expect("status should succeed on a dirty tree");

    assert!(!status.is_clean());
    let by_path = |name: &str| {
        status
            .files
            .iter()
            .find(|f| f.path == Path::new(name))
            .unwrap_or_else(|| panic!("expected a status entry for {name}"))
    };

    let staged = by_path("staged.txt");
    assert_eq!(staged.change_type, ChangeType::Modified);

    let unstaged = by_path("unstaged.txt");
    assert_eq!(unstaged.change_type, ChangeType::Modified);

    let untracked = by_path("untracked.txt");
    assert_eq!(untracked.change_type, ChangeType::Untracked);

    assert_eq!(status.files.len(), 3);
}

// ---------------------------------------------------------------------
// Rename detection via `diff`.
// ---------------------------------------------------------------------

#[test]
fn diff_detects_a_rename() {
    let repo_dir = init_repo("rename-diff");
    write_file(
        repo_dir.path(),
        "original.txt",
        "line one\nline two\nline three\nline four\nline five\n",
    );
    commit_all(repo_dir.path(), "first commit");

    git(repo_dir.path(), &["mv", "original.txt", "renamed.txt"]);
    write_file(
        repo_dir.path(),
        "renamed.txt",
        "line one\nline TWO\nline three\nline four\nline five\n",
    );
    git(repo_dir.path(), &["add", "-A"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // The rename is staged, not committed, so working-tree-vs-index is
    // empty; diff the index against HEAD (`from: Some(HEAD), to: None`) to
    // see it.
    let head_hash = head_commit_hash(repo_dir.path());
    let staged_diff = provider
        .diff(
            &repo,
            &DiffRequest {
                from: Some(head_hash),
                to: None,
                path_filter: None,
                context_lines: None,
            },
        )
        .expect("diff against HEAD should succeed");

    assert_eq!(staged_diff.files.len(), 1);
    let file = &staged_diff.files[0];
    assert_eq!(file.change_type, ChangeType::Renamed);
    assert_eq!(file.path, Path::new("renamed.txt"));
    assert_eq!(file.previous_path.as_deref(), Some(Path::new("original.txt")));
    assert!(!file.is_binary);
    assert!(!file.hunks.is_empty());
}

fn head_commit_hash(dir: &Path) -> CommitHash {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    let hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
    CommitHash::new(hash).unwrap()
}

// ---------------------------------------------------------------------
// Diff: modified file and binary file.
// ---------------------------------------------------------------------

#[test]
fn diff_reports_modified_content_hunks_between_two_commits() {
    let repo_dir = init_repo("modified-diff");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\nthree\n");
    commit_all(repo_dir.path(), "first commit");
    let first = head_commit_hash(repo_dir.path());

    write_file(repo_dir.path(), "a.txt", "one\nTWO\nthree\nfour\n");
    commit_all(repo_dir.path(), "second commit");
    let second = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let diff = provider
        .diff(
            &repo,
            &DiffRequest {
                from: Some(first),
                to: Some(second),
                path_filter: None,
                context_lines: None,
            },
        )
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.change_type, ChangeType::Modified);
    assert_eq!(file.path, Path::new("a.txt"));
    assert_eq!(file.hunks.len(), 1);
    let hunk = &file.hunks[0];
    assert!(hunk
        .lines
        .iter()
        .any(|l| l.content == "TWO" && l.origin == gitsail_domain::DiffLineOrigin::Addition));
    assert!(hunk
        .lines
        .iter()
        .any(|l| l.content == "two" && l.origin == gitsail_domain::DiffLineOrigin::Deletion));
}

// ---------------------------------------------------------------------
// Paginated history.
// ---------------------------------------------------------------------

#[test]
fn commit_history_is_paginated_with_a_cursor() {
    let repo_dir = init_repo("paginated-history");
    for i in 0..5 {
        write_file(repo_dir.path(), "f.txt", &format!("content {i}\n"));
        commit_all(repo_dir.path(), &format!("commit {i}"));
    }

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let first_page = provider
        .commits(
            &repo,
            &CommitQuery {
                limit: Some(2),
                ..CommitQuery::default()
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.items.len(), 2);
    assert!(first_page.has_more);
    let cursor = first_page
        .next_cursor
        .clone()
        .expect("has_more implies a next_cursor");

    // Most recent commit first: "commit 4" then "commit 3".
    assert_eq!(first_page.items[0].subject, "commit 4");
    assert_eq!(first_page.items[1].subject, "commit 3");

    let second_page = provider
        .commits(
            &repo,
            &CommitQuery {
                limit: Some(2),
                cursor: Some(cursor),
                ..CommitQuery::default()
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.items.len(), 2);
    assert!(second_page.has_more);
    assert_eq!(second_page.items[0].subject, "commit 2");
    assert_eq!(second_page.items[1].subject, "commit 1");

    let third_page = provider
        .commits(
            &repo,
            &CommitQuery {
                limit: Some(2),
                cursor: second_page.next_cursor.clone(),
                ..CommitQuery::default()
            },
        )
        .expect("third page should succeed");
    assert_eq!(third_page.items.len(), 1);
    assert!(!third_page.has_more);
    assert_eq!(third_page.next_cursor, None);
    assert_eq!(third_page.items[0].subject, "commit 0");
}

// ---------------------------------------------------------------------
// Blame.
// ---------------------------------------------------------------------

#[test]
fn blame_attributes_each_line_to_the_commit_that_introduced_it() {
    let repo_dir = init_repo("blame");
    write_file(repo_dir.path(), "a.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "first commit");

    write_file(repo_dir.path(), "a.txt", "line1changed\nline2\n");
    commit_all(repo_dir.path(), "second commit");
    let second = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let blame = provider
        .blame(&repo, Path::new("a.txt"), None)
        .expect("blame should succeed");

    assert_eq!(blame.lines.len(), 2);
    assert_eq!(blame.lines[0].content, "line1changed");
    assert_eq!(blame.lines[0].commit, second);
    assert_eq!(blame.lines[0].author.name, "Test User");
    assert_eq!(blame.lines[1].content, "line2");
    assert_ne!(blame.lines[1].commit, second);
}
