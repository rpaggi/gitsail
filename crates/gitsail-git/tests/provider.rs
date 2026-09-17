//! Integration tests for [`GitCliProvider`] (SAD §31): every test creates a
//! real, temporary Git repository via the `git` CLI (never a mock) and
//! exercises `GitCliProvider` against it through `RepositoryReadPort` /
//! `RepositoryWritePort`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    CommitQuery, CompareRevisions, DiffRequest, GetCommitDiff, RepositoryReadPort,
    RepositoryWritePort,
};
use gitsail_domain::{
    BranchKind, BranchName, CancellationToken, ChangeType, CommitHash, Decoration, DiffHunk,
    DiffLine, DiffLineOrigin, ErrorCode, FileDiff, FileStatusCode, HeadState,
};
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

fn write_bytes(dir: &Path, name: &str, contents: &[u8]) {
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

#[test]
fn commit_lookup_resolves_an_unambiguous_short_hash() {
    let repo_dir = init_repo("short-hash-lookup");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let full_hash = head_commit_hash(repo_dir.path());
    let short = full_hash.to_short(12);

    let commit = provider
        .commit(&repo, &CommitHash::new(short.as_str().to_string()).unwrap())
        .expect("an unambiguous short hash should resolve to the full commit");

    assert_eq!(commit.hash, full_hash);
}

#[test]
fn commit_history_and_lookup_preserve_all_parents_of_a_merge_commit() {
    let repo_dir = init_repo("merge-history");
    write_file(repo_dir.path(), "a.txt", "base\n");
    commit_all(repo_dir.path(), "base commit");
    let base = head_commit_hash(repo_dir.path());

    git(repo_dir.path(), &["checkout", "--quiet", "-b", "feature"]);
    write_file(repo_dir.path(), "feature.txt", "feature\n");
    commit_all(repo_dir.path(), "feature commit");
    let feature_tip = head_commit_hash(repo_dir.path());

    git(repo_dir.path(), &["checkout", "--quiet", "main"]);
    write_file(repo_dir.path(), "main.txt", "main\n");
    commit_all(repo_dir.path(), "main commit");

    git(
        repo_dir.path(),
        &["merge", "--quiet", "--no-ff", "-m", "merge feature into main", "feature"],
    );
    let merge_hash = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(&repo, &CommitQuery::default())
        .expect("commits should succeed");
    let merge_commit = page
        .items
        .iter()
        .find(|c| c.hash == merge_hash)
        .expect("merge commit should be present in the history");
    assert!(merge_commit.is_merge());
    assert!(!merge_commit.is_root());
    assert_eq!(merge_commit.parents.len(), 2);
    assert!(merge_commit.parents.contains(&feature_tip));
    assert!(!merge_commit.parents.contains(&base));

    let fetched = provider
        .commit(&repo, &merge_hash)
        .expect("fetching the merge commit by hash should succeed");
    assert_eq!(fetched.parents, merge_commit.parents);
}

// ---------------------------------------------------------------------
// US-006: current dir, root and subdirectory resolve the same repository;
// nonexistent/non-repository paths fail without mutating anything.
// ---------------------------------------------------------------------

#[test]
fn opening_from_root_or_a_subdirectory_resolves_the_same_repository() {
    let repo_dir = init_repo("open-subdir");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    std::fs::create_dir_all(repo_dir.path().join("nested/deeper")).unwrap();
    write_file(&repo_dir.path().join("nested/deeper"), "b.txt", "hi\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let from_root = provider.discover(repo_dir.path()).unwrap();
    let from_subdir = provider
        .discover(&repo_dir.path().join("nested"))
        .unwrap();
    let from_deeper = provider
        .discover(&repo_dir.path().join("nested/deeper"))
        .unwrap();

    assert_eq!(from_root.id, from_subdir.id);
    assert_eq!(from_root.id, from_deeper.id);
    assert_eq!(from_root.root_path, from_subdir.root_path);
    assert_eq!(from_root.root_path, from_deeper.root_path);
    assert_eq!(from_subdir.head_state, from_root.head_state);
}

#[test]
fn opening_a_linked_worktree_does_not_assume_dot_git_is_a_directory() {
    let repo_dir = init_repo("open-linked-worktree");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    let worktree_dir = TempDir::new("open-linked-worktree-wt");
    // Remove the freshly created dir so `git worktree add` can create it.
    std::fs::remove_dir_all(worktree_dir.path()).unwrap();
    git(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "wt-branch",
            worktree_dir.path().to_str().unwrap(),
        ],
    );
    // A linked worktree's `.git` is a *file* pointing at the main
    // repository's gitdir, not a directory — proving discovery does not
    // hardcode that assumption.
    assert!(worktree_dir.path().join(".git").is_file());

    let provider = provider();
    let repo = provider
        .discover(worktree_dir.path())
        .expect("discovering a linked worktree should succeed");

    assert!(!repo.is_bare);
    assert_eq!(repo.worktree_path.as_deref(), Some(repo.root_path.as_path()));
    match &repo.head_state {
        HeadState::Attached { branch } => assert_eq!(branch.as_str(), "wt-branch"),
        other => panic!("expected Attached, got {other:?}"),
    }
}

#[test]
fn discovering_a_nonexistent_path_fails_without_creating_anything() {
    let parent = TempDir::new("open-missing-parent");
    let missing = parent.path().join("does-not-exist");
    let before: Vec<_> = std::fs::read_dir(parent.path()).unwrap().collect();
    assert!(before.is_empty());

    let provider = provider();
    let err = provider
        .discover(&missing)
        .expect_err("discovering a nonexistent path should fail");
    assert!(err.diagnostic().is_some() || !err.message().is_empty());

    let after: Vec<_> = std::fs::read_dir(parent.path()).unwrap().collect();
    assert!(
        after.is_empty(),
        "discover must never create files/directories on failure"
    );
}

#[test]
fn discovering_a_directory_outside_any_repository_reports_repository_not_found() {
    let dir = TempDir::new("open-non-repo");
    let before: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(before.is_empty());

    let provider = provider();
    let err = provider
        .discover(dir.path())
        .expect_err("discovering a non-Git directory should fail");
    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);

    let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(
        after.is_empty(),
        "discover must never create files/directories on failure"
    );
}

// ---------------------------------------------------------------------
// US-008: paths are preserved verbatim — Unicode and spaces in both the
// repository directory itself and in tracked file paths.
// ---------------------------------------------------------------------

#[test]
fn opens_a_repository_whose_own_path_contains_unicode_and_spaces() {
    let parent = TempDir::new("unicode-parent");
    let repo_path = parent.path().join("Projeto Ação ☃ café");
    std::fs::create_dir_all(&repo_path).unwrap();
    git(&repo_path, &["init", "--quiet", "--initial-branch=main"]);
    git(&repo_path, &["config", "user.name", "Test User"]);
    git(&repo_path, &["config", "user.email", "test@example.com"]);
    write_file(&repo_path, "a.txt", "hello\n");
    commit_all(&repo_path, "first commit");

    let provider = provider();
    let repo = provider
        .discover(&repo_path)
        .expect("discover should succeed for a Unicode, space-containing repo path");

    assert_eq!(repo.root_path, repo_path.canonicalize().unwrap());
    let status = provider.status(&repo).expect("status should succeed");
    assert!(status.is_clean());
}

#[test]
fn preserves_unicode_and_space_containing_file_paths_in_status_and_diff() {
    let repo_dir = init_repo("unicode-file-paths");
    let filename = "arquivo com espaço e açúcar ☃.txt";
    write_file(repo_dir.path(), filename, "conteúdo inicial\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), filename, "conteúdo alterado\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let status = provider.status(&repo).expect("status should succeed");
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].path, Path::new(filename));

    let diff = provider
        .diff(&repo, &DiffRequest::default(), &CancellationToken::new())
        .expect("diff should succeed");
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, Path::new(filename));
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

#[test]
fn branches_on_an_unborn_repository_is_empty() {
    let repo_dir = init_repo("unborn-branches");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let branches = provider
        .branches(&repo)
        .expect("branches should succeed (as an empty list) on an unborn repository");

    assert!(branches.is_empty(), "an unborn repository must not report a fictitious branch");
}

#[test]
fn branches_while_detached_report_no_current_branch() {
    let repo_dir = init_repo("detached-branches");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    let target = head_commit_hash(repo_dir.path());
    git(repo_dir.path(), &["checkout", "--quiet", target.as_str()]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let branches = provider
        .branches(&repo)
        .expect("branches should succeed while detached");

    assert!(branches.iter().any(|b| b.name.as_str() == "main"));
    assert!(
        branches.iter().all(|b| !b.is_current),
        "a detached HEAD must not mark any branch as current"
    );
}

#[test]
fn branches_report_upstream_and_ahead_behind_when_calculable_and_absence_differs_from_zero() {
    let remote_dir = init_bare_repo("branch-upstream-remote");
    let repo_dir = init_repo("branch-upstream-local");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "commit a");
    let commit_a = head_commit_hash(repo_dir.path());
    git(
        repo_dir.path(),
        &["remote", "add", "origin", remote_dir.path().to_str().unwrap()],
    );
    git(repo_dir.path(), &["push", "--quiet", "-u", "origin", "main"]);
    // A local-only branch, deliberately left without an upstream.
    git(repo_dir.path(), &["branch", "feature", commit_a.as_str()]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let branches = provider.branches(&repo).expect("branches should succeed");

    let main = branches.iter().find(|b| b.name.as_str() == "main").unwrap();
    assert_eq!(
        main.upstream.as_ref().map(BranchName::as_str),
        Some("origin/main")
    );
    assert_eq!((main.ahead, main.behind), (0, 0));

    let feature = branches.iter().find(|b| b.name.as_str() == "feature").unwrap();
    assert_eq!(
        feature.upstream, None,
        "a branch with no upstream must report None, not merely 0/0 ahead/behind"
    );
    assert_eq!((feature.ahead, feature.behind), (0, 0));

    // Push a second commit so the remote-tracking ref moves past commit a.
    write_file(repo_dir.path(), "a.txt", "hello again\n");
    commit_all(repo_dir.path(), "commit b (pushed)");
    git(repo_dir.path(), &["push", "--quiet"]);

    // Reset the local branch back to commit a without touching the
    // remote-tracking ref: `main` is now purely behind its upstream.
    git(repo_dir.path(), &["reset", "--quiet", "--hard", commit_a.as_str()]);
    let branches = provider.branches(&repo).unwrap();
    let main = branches.iter().find(|b| b.name.as_str() == "main").unwrap();
    assert_eq!((main.ahead, main.behind), (0, 1));

    // A new, unpushed commit from commit a diverges from the pushed commit
    // b: `main` is now both ahead and behind.
    write_file(repo_dir.path(), "a.txt", "a different local history\n");
    commit_all(repo_dir.path(), "commit c (diverges from pushed commit b)");
    let branches = provider.branches(&repo).unwrap();
    let main = branches.iter().find(|b| b.name.as_str() == "main").unwrap();
    assert_eq!((main.ahead, main.behind), (1, 1));
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

#[test]
fn commit_history_is_available_while_head_is_detached() {
    let repo_dir = init_repo("detached-history");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "hello again\n");
    commit_all(repo_dir.path(), "second commit");
    let target = head_commit_hash(repo_dir.path());
    git(repo_dir.path(), &["checkout", "--quiet", target.as_str()]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    assert!(matches!(repo.head_state, HeadState::Detached { .. }));

    let page = provider
        .commits(&repo, &CommitQuery::default())
        .expect("commit history should succeed while HEAD is detached");

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].hash, target);
    assert_eq!(page.items[0].subject, "second commit");
    assert_eq!(page.items[1].subject, "first commit");
}

// ---------------------------------------------------------------------
// Bare repository.
// ---------------------------------------------------------------------

fn init_bare_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(dir.path(), &["init", "--quiet", "--bare", "--initial-branch=main"]);
    dir
}

#[test]
fn discovers_a_bare_repository_and_reports_no_worktree() {
    let repo_dir = init_bare_repo("bare-discover");
    let provider = provider();

    let repo = provider
        .discover(repo_dir.path())
        .expect("discover should succeed for a bare repository");

    assert!(repo.is_bare);
    assert_eq!(repo.worktree_path, None);
    assert_eq!(repo.head_state, HeadState::Unborn);
}

#[test]
fn status_on_a_bare_repository_reports_an_explicit_limitation() {
    let repo_dir = init_bare_repo("bare-status");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .status(&repo)
        .expect_err("status must reject a bare repository explicitly");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert!(err.remediation().is_some());
}

#[test]
fn working_tree_diff_on_a_bare_repository_reports_an_explicit_limitation() {
    let repo_dir = init_bare_repo("bare-diff");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .diff(&repo, &DiffRequest::default(), &CancellationToken::new())
        .expect_err("a working-tree diff must reject a bare repository explicitly");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
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

#[test]
fn status_combines_staged_and_unstaged_changes_on_the_same_path_with_an_unusual_name() {
    // US-010 DoD: a single fixture combining a staged-and-unstaged change on
    // the same file, and an unusual (non-alphanumeric) path.
    let name = "weird name (v1) - final.txt";
    let repo_dir = init_repo("same-path-combo");
    write_file(repo_dir.path(), name, "original\n");
    commit_all(repo_dir.path(), "first commit");

    // Stage one change...
    write_file(repo_dir.path(), name, "staged change\n");
    git(repo_dir.path(), &["add", name]);
    // ...then modify again without staging.
    write_file(repo_dir.path(), name, "unstaged change on top\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let status = provider
        .status(&repo)
        .expect("status should succeed with a combined staged/unstaged change");

    assert_eq!(status.files.len(), 1);
    let entry = &status.files[0];
    assert_eq!(entry.path, Path::new(name));
    assert_eq!(entry.index_status, FileStatusCode::Modified);
    assert_eq!(entry.worktree_status, FileStatusCode::Modified);
    assert!(!status.is_clean());
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
                staged: false,
                path_filter: None,
                context_lines: None,
            },
            &CancellationToken::new(),
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
                staged: false,
                path_filter: None,
                context_lines: None,
            },
            &CancellationToken::new(),
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

#[test]
fn diff_reports_a_binary_file_change_without_fabricating_hunks() {
    let repo_dir = init_repo("binary-diff");
    write_bytes(repo_dir.path(), "image.png", &[0x89, b'P', b'N', b'G', 0x00, 0x01]);
    commit_all(repo_dir.path(), "add binary");

    write_bytes(repo_dir.path(), "image.png", &[0x89, b'P', b'N', b'G', 0x02, 0x03, 0x04]);
    commit_all(repo_dir.path(), "change binary");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let diff = provider
        .diff(&repo, &unstaged_diff_request(), &CancellationToken::new())
        .unwrap();

    // Nothing is unstaged (both changes were committed); diff HEAD~1..HEAD instead.
    assert!(diff.files.is_empty());

    let first = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(repo_dir.path())
            .output()
            .unwrap();
        CommitHash::new(String::from_utf8(output.stdout).unwrap().trim().to_string()).unwrap()
    };
    let second = head_commit_hash(repo_dir.path());

    let diff = provider
        .diff(
            &repo,
            &DiffRequest {
                from: Some(first),
                to: Some(second),
                staged: false,
                path_filter: None,
                context_lines: None,
            },
            &CancellationToken::new(),
        )
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert!(file.is_binary);
    assert!(
        file.hunks.is_empty(),
        "a binary diff must never contain fabricated textual hunks"
    );
    assert_eq!(file.path, Path::new("image.png"));
}

#[test]
fn diff_preserves_crlf_line_endings_in_line_content() {
    let repo_dir = init_repo("crlf-diff");
    write_bytes(repo_dir.path(), "a.txt", b"one\r\ntwo\r\nthree\r\n");
    git(repo_dir.path(), &["add", "-A"]);
    git(repo_dir.path(), &["commit", "--quiet", "-m", "first commit"]);

    write_bytes(repo_dir.path(), "a.txt", b"one\r\nTWO\r\nthree\r\n");
    git(repo_dir.path(), &["add", "-A"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let diff = provider
        .diff(&repo, &staged_diff_request(), &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let hunk = &diff.files[0].hunks[0];
    assert!(hunk
        .lines
        .iter()
        .any(|l| l.content == "TWO\r" && l.origin == DiffLineOrigin::Addition));
    assert!(hunk
        .lines
        .iter()
        .any(|l| l.content == "two\r" && l.origin == DiffLineOrigin::Deletion));
}

#[test]
fn diff_marks_a_line_with_no_trailing_newline_instead_of_inventing_one() {
    let repo_dir = init_repo("no-trailing-newline-diff");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\n");
    commit_all(repo_dir.path(), "first commit");

    // No trailing `\n` after "three".
    std::fs::write(repo_dir.path().join("a.txt"), "one\ntwo\nthree").unwrap();
    git(repo_dir.path(), &["add", "-A"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let diff = provider
        .diff(&repo, &staged_diff_request(), &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let hunk = &diff.files[0].hunks[0];
    let added = hunk
        .lines
        .iter()
        .find(|l| l.content == "three" && l.origin == DiffLineOrigin::Addition)
        .expect("the added line without a trailing newline must still be reported");
    assert!(!added.has_trailing_newline);
}

#[test]
fn diff_truncates_a_file_whose_content_exceeds_the_size_limit() {
    let repo_dir = init_repo("large-file-diff");
    let small_content: String = (0..10).map(|i| format!("line {i}\n")).collect();
    write_file(repo_dir.path(), "a.txt", &small_content);
    commit_all(repo_dir.path(), "first commit");

    // Well beyond the adapter's 512 KiB per-file hunk cap.
    let large_content: String = (0..40_000).map(|i| format!("line {i} of a very large file\n")).collect();
    write_file(repo_dir.path(), "a.txt", &large_content);
    commit_all(repo_dir.path(), "grow the file");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let first = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(repo_dir.path())
            .output()
            .unwrap();
        CommitHash::new(String::from_utf8(output.stdout).unwrap().trim().to_string()).unwrap()
    };
    let second = head_commit_hash(repo_dir.path());

    let diff = provider
        .diff(
            &repo,
            &DiffRequest {
                from: Some(first),
                to: Some(second),
                staged: false,
                path_filter: None,
                context_lines: None,
            },
            &CancellationToken::new(),
        )
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert!(
        file.truncated,
        "a file diff far beyond the size cap must be reported as truncated"
    );
    assert!(
        file.hunks.is_empty(),
        "hunks are withheld, not partially/incorrectly parsed, once truncated"
    );
}

#[test]
fn diff_fails_with_cancelled_when_the_token_is_already_cancelled() {
    let repo_dir = init_repo("cancelled-diff");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "one\nTWO\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let err = provider
        .diff(&repo, &unstaged_diff_request(), &cancel)
        .expect_err("a pre-cancelled token must stop the diff before it succeeds");

    assert_eq!(err.code(), ErrorCode::Cancelled);
}

#[test]
fn unstaged_and_staged_diffs_are_distinct_for_the_same_file() {
    let repo_dir = init_repo("staged-and-unstaged-diff");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\nthree\n");
    commit_all(repo_dir.path(), "first commit");

    // Stage one change, then make a further, different change on top of it.
    write_file(repo_dir.path(), "a.txt", "one\nTWO\nthree\n");
    git(repo_dir.path(), &["add", "-A"]);
    write_file(repo_dir.path(), "a.txt", "one\nTWO\nthree\nfour\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let staged = provider
        .diff(&repo, &staged_diff_request(), &CancellationToken::new())
        .unwrap();
    let unstaged = provider
        .diff(&repo, &unstaged_diff_request(), &CancellationToken::new())
        .unwrap();

    assert_eq!(staged.files.len(), 1);
    assert!(staged.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "TWO" && l.origin == DiffLineOrigin::Addition));

    assert_eq!(unstaged.files.len(), 1);
    assert!(unstaged.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "four" && l.origin == DiffLineOrigin::Addition));
    assert!(
        !unstaged.files[0]
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.content == "TWO"
                && (l.origin == DiffLineOrigin::Addition || l.origin == DiffLineOrigin::Deletion)),
        "the already-staged change must not appear as an addition/deletion in the unstaged \
         comparison (it may still appear as unchanged context)"
    );
}

// ---------------------------------------------------------------------
// Revision resolution (US-028).
// ---------------------------------------------------------------------

#[test]
fn resolve_revision_resolves_branches_and_relative_refs() {
    let repo_dir = init_repo("resolve-revision");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let first = head_commit_hash(repo_dir.path());
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "second commit");
    let second = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    assert_eq!(provider.resolve_revision(&repo, "main").unwrap(), second);
    assert_eq!(provider.resolve_revision(&repo, "HEAD~1").unwrap(), first);
    assert_eq!(
        provider
            .resolve_revision(&repo, second.as_str())
            .unwrap(),
        second
    );
}

#[test]
fn resolve_revision_fails_clearly_for_an_invalid_reference() {
    let repo_dir = init_repo("resolve-revision-invalid");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .resolve_revision(&repo, "does-not-exist")
        .expect_err("an unresolvable revision must fail rather than default to something");

    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
}

// ---------------------------------------------------------------------
// Commit diff and revision comparison use cases (US-026, US-028).
// ---------------------------------------------------------------------

#[test]
fn commit_diff_of_a_root_commit_uses_the_empty_tree_as_its_base() {
    let repo_dir = init_repo("commit-diff-root");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "root commit");
    let root = head_commit_hash(repo_dir.path());

    let provider = Arc::new(provider());
    let repo = provider.discover(repo_dir.path()).unwrap();
    let use_case = GetCommitDiff::new(provider.clone());

    let result = use_case
        .execute(&repo, &root, &CancellationToken::new())
        .unwrap();

    assert!(result.base.is_none(), "a root commit has no parent to name as base");
    assert_eq!(result.diff.files.len(), 1);
    assert_eq!(result.diff.files[0].change_type, ChangeType::Added);
    assert_eq!(result.diff.files[0].path, Path::new("a.txt"));
}

#[test]
fn commit_diff_of_a_merge_commit_uses_the_first_parent() {
    let repo_dir = init_repo("commit-diff-merge");
    write_file(repo_dir.path(), "base.txt", "base\n");
    commit_all(repo_dir.path(), "base commit");

    git(repo_dir.path(), &["checkout", "-b", "feature"]);
    write_file(repo_dir.path(), "feature.txt", "feature\n");
    commit_all(repo_dir.path(), "feature commit");

    git(repo_dir.path(), &["checkout", "main"]);
    write_file(repo_dir.path(), "main.txt", "main\n");
    commit_all(repo_dir.path(), "main commit");
    // The merge commit's first parent is `main`'s tip at merge time, i.e.
    // this commit, not the earlier "base commit".
    let main_tip = head_commit_hash(repo_dir.path());

    git(repo_dir.path(), &["merge", "--no-ff", "--quiet", "-m", "merge feature", "feature"]);
    let merge = head_commit_hash(repo_dir.path());

    let provider = Arc::new(provider());
    let repo = provider.discover(repo_dir.path()).unwrap();
    let use_case = GetCommitDiff::new(provider.clone());

    let result = use_case
        .execute(&repo, &merge, &CancellationToken::new())
        .unwrap();

    assert_eq!(
        result.base,
        Some(main_tip),
        "the merge diff base must be the first parent (main's tip), not the merged-in branch"
    );
    // First-parent diff (base = main's tip, target = the merge commit) shows
    // exactly what the merge introduced beyond `main`: `feature.txt`.
    // `main.txt` was already present at `main`'s tip, so it is unchanged
    // and does not appear.
    let paths: Vec<_> = result.diff.files.iter().map(|f| f.path.clone()).collect();
    assert!(paths.contains(&PathBuf::from("feature.txt")));
    assert!(!paths.contains(&PathBuf::from("main.txt")));
}

#[test]
fn compare_revisions_reverses_signs_and_paths_when_sides_are_swapped() {
    let repo_dir = init_repo("compare-revisions-swap");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "one\nTWO\n");
    commit_all(repo_dir.path(), "second commit");

    let provider = Arc::new(provider());
    let repo = provider.discover(repo_dir.path()).unwrap();
    let use_case = CompareRevisions::new(provider.clone());

    let forward = use_case
        .execute(&repo, "HEAD~1", "HEAD", &CancellationToken::new())
        .unwrap();
    let forward_hunk = &forward.diff.files[0].hunks[0];
    assert!(forward_hunk
        .lines
        .iter()
        .any(|l| l.content == "TWO" && l.origin == DiffLineOrigin::Addition));

    let reversed = use_case
        .execute(&repo, "HEAD", "HEAD~1", &CancellationToken::new())
        .unwrap();
    let reversed_hunk = &reversed.diff.files[0].hunks[0];
    assert!(reversed_hunk
        .lines
        .iter()
        .any(|l| l.content == "TWO" && l.origin == DiffLineOrigin::Deletion));
    assert!(reversed_hunk
        .lines
        .iter()
        .any(|l| l.content == "two" && l.origin == DiffLineOrigin::Addition));
    assert_eq!(forward.base, reversed.target);
    assert_eq!(forward.target, reversed.base);
}

#[test]
fn compare_revisions_fails_clearly_for_an_invalid_reference() {
    let repo_dir = init_repo("compare-revisions-invalid");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = Arc::new(provider());
    let repo = provider.discover(repo_dir.path()).unwrap();
    let use_case = CompareRevisions::new(provider.clone());

    let err = use_case
        .execute(&repo, "does-not-exist", "HEAD", &CancellationToken::new())
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
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
// US-017: filtering and searching history.
// ---------------------------------------------------------------------

#[test]
fn commit_history_filters_by_author() {
    let repo_dir = init_repo("filter-by-author");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "commit by test user");
    git(repo_dir.path(), &["config", "user.name", "Someone Else"]);
    git(repo_dir.path(), &["config", "user.email", "else@example.com"]);
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "commit by someone else");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                author: Some("Someone Else".to_string()),
                ..CommitQuery::default()
            },
        )
        .expect("author-filtered history should succeed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].subject, "commit by someone else");
}

#[test]
fn commit_history_filters_by_text_query() {
    let repo_dir = init_repo("filter-by-text");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "fix the login bug");
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "add new feature");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                text_query: Some("login".to_string()),
                ..CommitQuery::default()
            },
        )
        .expect("text-filtered history should succeed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].subject, "fix the login bug");
}

#[test]
fn commit_history_filter_with_zero_matches_is_an_empty_page_not_an_error() {
    let repo_dir = init_repo("filter-zero-results");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "an ordinary commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                text_query: Some("no-commit-matches-this".to_string()),
                ..CommitQuery::default()
            },
        )
        .expect("zero matches must not be reported as an error");

    assert!(page.items.is_empty());
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn commit_history_combines_a_filter_with_pagination() {
    let repo_dir = init_repo("filter-with-pagination");
    for i in 0..4 {
        write_file(repo_dir.path(), "a.txt", &format!("content {i}\n"));
        commit_all(repo_dir.path(), &format!("relevant commit {i}"));
        write_file(repo_dir.path(), "b.txt", &format!("content {i}\n"));
        commit_all(repo_dir.path(), &format!("unrelated commit {i}"));
    }

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let query = CommitQuery {
        text_query: Some("relevant".to_string()),
        limit: Some(2),
        ..CommitQuery::default()
    };

    let first_page = provider.commits(&repo, &query).expect("first filtered page should succeed");
    assert_eq!(first_page.items.len(), 2);
    assert!(first_page.has_more);
    assert!(first_page.items.iter().all(|c| c.subject.starts_with("relevant")));

    let second_page = provider
        .commits(
            &repo,
            &CommitQuery {
                cursor: first_page.next_cursor.clone(),
                ..query
            },
        )
        .expect("second filtered page should succeed");
    assert_eq!(second_page.items.len(), 2);
    assert!(!second_page.has_more);
    assert!(second_page.items.iter().all(|c| c.subject.starts_with("relevant")));
}

// ---------------------------------------------------------------------
// US-018: file history.
// ---------------------------------------------------------------------

#[test]
fn file_history_includes_only_commits_touching_the_path() {
    let repo_dir = init_repo("file-history-basic");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "touch a");
    write_file(repo_dir.path(), "b.txt", "one\n");
    commit_all(repo_dir.path(), "touch b");
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "touch a again");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                path_filter: Some(PathBuf::from("a.txt")),
                ..CommitQuery::default()
            },
        )
        .expect("file history should succeed");

    let subjects: Vec<&str> = page.items.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["touch a again", "touch a"]);
}

#[test]
fn file_history_follows_renames_when_requested_and_stops_at_the_boundary_otherwise() {
    let repo_dir = init_repo("file-history-rename");
    write_file(repo_dir.path(), "old.txt", "content\n");
    commit_all(repo_dir.path(), "create old.txt");
    git(repo_dir.path(), &["mv", "old.txt", "new.txt"]);
    commit_all(repo_dir.path(), "rename to new.txt");
    write_file(repo_dir.path(), "new.txt", "content changed\n");
    commit_all(repo_dir.path(), "edit new.txt");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let followed = provider
        .commits(
            &repo,
            &CommitQuery {
                path_filter: Some(PathBuf::from("new.txt")),
                follow_renames: true,
                ..CommitQuery::default()
            },
        )
        .expect("followed file history should succeed");
    let followed_subjects: Vec<&str> = followed.items.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        followed_subjects,
        vec!["edit new.txt", "rename to new.txt", "create old.txt"],
        "following renames must surface history under the file's former name"
    );

    let not_followed = provider
        .commits(
            &repo,
            &CommitQuery {
                path_filter: Some(PathBuf::from("new.txt")),
                follow_renames: false,
                ..CommitQuery::default()
            },
        )
        .expect("non-followed file history should succeed");
    let not_followed_subjects: Vec<&str> =
        not_followed.items.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        not_followed_subjects,
        vec!["edit new.txt", "rename to new.txt"],
        "without following renames, history must stop at the rename boundary"
    );
}

#[test]
fn file_history_for_a_removed_file_stops_at_its_deletion_without_inventing_later_commits() {
    let repo_dir = init_repo("file-history-removed");
    write_file(repo_dir.path(), "gone.txt", "content\n");
    commit_all(repo_dir.path(), "create gone.txt");
    git(repo_dir.path(), &["rm", "--quiet", "gone.txt"]);
    commit_all(repo_dir.path(), "delete gone.txt");
    write_file(repo_dir.path(), "other.txt", "unrelated\n");
    commit_all(repo_dir.path(), "unrelated later commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                path_filter: Some(PathBuf::from("gone.txt")),
                ..CommitQuery::default()
            },
        )
        .expect("file history for a removed file should succeed");

    let subjects: Vec<&str> = page.items.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["delete gone.txt", "create gone.txt"]);
}

#[test]
fn file_history_for_a_path_with_no_history_is_an_empty_page_not_an_error() {
    let repo_dir = init_repo("file-history-none");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "unrelated commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let page = provider
        .commits(
            &repo,
            &CommitQuery {
                path_filter: Some(PathBuf::from("never-existed.txt")),
                ..CommitQuery::default()
            },
        )
        .expect("a path with no history must not be reported as an error");

    assert!(page.items.is_empty());
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

// ---------------------------------------------------------------------
// Stage / unstage by file (US-011).
// ---------------------------------------------------------------------

fn unstaged_diff_request() -> DiffRequest {
    DiffRequest {
        from: None,
        to: None,
        staged: false,
        path_filter: None,
        context_lines: None,
    }
}

fn staged_diff_request() -> DiffRequest {
    DiffRequest {
        from: None,
        to: None,
        staged: true,
        path_filter: None,
        context_lines: None,
    }
}

#[test]
fn stage_files_stages_only_the_selected_paths_including_a_removal() {
    let repo_dir = init_repo("stage-selected");
    write_file(repo_dir.path(), "keep.txt", "keep\n");
    write_file(repo_dir.path(), "remove.txt", "remove me\n");
    commit_all(repo_dir.path(), "first commit");

    write_file(repo_dir.path(), "keep.txt", "keep changed\n");
    std::fs::remove_file(repo_dir.path().join("remove.txt")).unwrap();
    write_file(repo_dir.path(), "untracked.txt", "new\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .stage_files(
            &repo,
            &[PathBuf::from("keep.txt"), PathBuf::from("remove.txt")],
        )
        .expect("staging selected paths should succeed");

    let status = provider.status(&repo).unwrap();
    let by_path = |name: &str| status.files.iter().find(|f| f.path == Path::new(name));

    let kept = by_path("keep.txt").expect("keep.txt should be staged");
    assert_eq!(kept.index_status, FileStatusCode::Modified);
    let removed = by_path("remove.txt").expect("remove.txt should be staged");
    assert_eq!(removed.index_status, FileStatusCode::Deleted);
    let untracked = by_path("untracked.txt").expect("untracked.txt was never selected");
    assert_eq!(untracked.change_type, ChangeType::Untracked);
}

#[test]
fn stage_files_works_before_the_first_commit() {
    let repo_dir = init_repo("stage-unborn");
    write_file(repo_dir.path(), "a.txt", "hello\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .stage_files(&repo, &[PathBuf::from("a.txt")])
        .expect("staging on an unborn branch should succeed");

    let status = provider.status(&repo).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].index_status, FileStatusCode::Added);
}

#[test]
fn stage_files_reports_failure_without_a_false_success_for_a_vanished_path() {
    let repo_dir = init_repo("stage-vanished");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .stage_files(&repo, &[PathBuf::from("never-existed.txt")])
        .expect_err("staging a path that never existed must fail, not silently succeed");

    assert_eq!(err.code(), ErrorCode::ProcessFailure);
}

#[test]
fn unstage_files_preserves_working_tree_content() {
    let repo_dir = init_repo("unstage-preserve");
    write_file(repo_dir.path(), "a.txt", "original\n");
    commit_all(repo_dir.path(), "first commit");

    write_file(repo_dir.path(), "a.txt", "staged change\n");
    git(repo_dir.path(), &["add", "a.txt"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .unstage_files(&repo, &[PathBuf::from("a.txt")])
        .expect("unstage should succeed");

    let status = provider.status(&repo).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].index_status, FileStatusCode::Unmodified);
    assert_eq!(status.files[0].worktree_status, FileStatusCode::Modified);
    let contents = std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap();
    assert_eq!(contents, "staged change\n", "unstage must not touch the working tree");
}

#[test]
fn unstage_files_works_before_the_first_commit() {
    let repo_dir = init_repo("unstage-unborn");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    git(repo_dir.path(), &["add", "a.txt"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .unstage_files(&repo, &[PathBuf::from("a.txt")])
        .expect("unstage on an unborn branch should succeed");

    let status = provider.status(&repo).unwrap();
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].change_type, ChangeType::Untracked);
    assert!(repo_dir.path().join("a.txt").exists());
}

#[test]
fn unstage_files_fully_unstages_a_rename_selected_by_only_the_new_path() {
    let repo_dir = init_repo("unstage-rename");
    write_file(repo_dir.path(), "old.txt", "content\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["mv", "old.txt", "new.txt"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let before = provider.status(&repo).unwrap();
    assert_eq!(before.files[0].change_type, ChangeType::Renamed);

    provider
        .unstage_files(&repo, &[PathBuf::from("new.txt")])
        .expect("unstage should succeed even selecting only the rename's new path");

    let status = provider.status(&repo).unwrap();
    let by_path = |name: &str| status.files.iter().find(|f| f.path == Path::new(name));

    let old = by_path("old.txt").expect("old.txt must be reported once fully unstaged");
    assert_eq!(old.index_status, FileStatusCode::Unmodified, "old.txt's index must fully match HEAD again, not stay staged as removed");
    assert_eq!(old.worktree_status, FileStatusCode::Deleted);
    let new = by_path("new.txt").expect("new.txt should be untracked again");
    assert_eq!(new.change_type, ChangeType::Untracked);
}

// ---------------------------------------------------------------------
// Commit from the index (US-012).
// ---------------------------------------------------------------------

#[test]
fn create_commit_returns_the_new_hash_and_advances_head() {
    let repo_dir = init_repo("commit-success");
    write_file(repo_dir.path(), "a.txt", "hello\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider.stage_files(&repo, &[PathBuf::from("a.txt")]).unwrap();

    let hash = provider
        .create_commit(&repo, "first commit")
        .expect("commit should succeed");

    let head = head_commit_hash(repo_dir.path());
    assert_eq!(hash, head);
    let status = provider.status(&repo).unwrap();
    assert!(status.is_clean());
}

#[test]
fn create_commit_rejects_an_empty_index_without_creating_a_commit() {
    let repo_dir = init_repo("commit-empty");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    let before = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .create_commit(&repo, "should not be created")
        .expect_err("an empty index must never be committed implicitly");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert_eq!(head_commit_hash(repo_dir.path()), before);
}

#[cfg(unix)]
#[test]
fn create_commit_on_hook_failure_preserves_the_staged_index() {
    use std::os::unix::fs::PermissionsExt;

    let repo_dir = init_repo("commit-hook-failure");
    let hooks_dir = repo_dir.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    std::fs::write(&hook_path, "#!/bin/sh\necho blocked by hook >&2\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&hook_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&hook_path, perms).unwrap();

    write_file(repo_dir.path(), "a.txt", "hello\n");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider.stage_files(&repo, &[PathBuf::from("a.txt")]).unwrap();

    let err = provider
        .create_commit(&repo, "blocked")
        .expect_err("a failing pre-commit hook must fail the commit");

    assert_eq!(err.code(), ErrorCode::ProcessFailure);
    assert!(err.diagnostic().unwrap().to_string().contains("blocked by hook"));

    let status = provider.status(&repo).unwrap();
    assert_eq!(status.files[0].index_status, FileStatusCode::Added, "the staged work must survive a failed commit");
}

// ---------------------------------------------------------------------
// Staged diff (`git diff --cached`), needed to unstage by hunk.
// ---------------------------------------------------------------------

#[test]
fn staged_diff_compares_the_index_against_head_independent_of_the_working_tree() {
    let repo_dir = init_repo("staged-diff");
    write_file(repo_dir.path(), "a.txt", "one\ntwo\nthree\n");
    commit_all(repo_dir.path(), "first commit");

    write_file(repo_dir.path(), "a.txt", "one\nTWO\nthree\n");
    git(repo_dir.path(), &["add", "a.txt"]);
    // Further unstaged change on top of the staged one.
    write_file(repo_dir.path(), "a.txt", "one\nTWO\nTHREE\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let staged = provider.diff(&repo, &staged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(staged.files.len(), 1);
    let staged_lines: Vec<_> = staged.files[0]
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin != DiffLineOrigin::Context)
        .map(|l| l.content.clone())
        .collect();
    assert_eq!(staged_lines, vec!["two".to_string(), "TWO".to_string()]);

    let unstaged = provider.diff(&repo, &unstaged_diff_request(), &CancellationToken::new()).unwrap();
    let unstaged_lines: Vec<_> = unstaged.files[0]
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin != DiffLineOrigin::Context)
        .map(|l| l.content.clone())
        .collect();
    assert_eq!(unstaged_lines, vec!["three".to_string(), "THREE".to_string()]);
}

// ---------------------------------------------------------------------
// Stage / unstage by hunk (US-013).
// ---------------------------------------------------------------------

/// 20 lines, so two edits far apart from each other produce two disjoint
/// `-U3` hunks rather than merging into one.
fn twenty_lines() -> String {
    (1..=20).map(|n| format!("l{n}\n")).collect()
}

fn two_far_apart_edits() -> String {
    let mut lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
    lines[2] = "L3-changed".to_string();
    lines[16] = "L17-changed".to_string();
    lines.join("\n") + "\n"
}

#[test]
fn stage_hunks_stages_only_the_selected_hunk() {
    let repo_dir = init_repo("stage-hunk-selected");
    write_file(repo_dir.path(), "a.txt", &twenty_lines());
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", &two_far_apart_edits());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let diff = provider.diff(&repo, &unstaged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].hunks.len(), 2, "the fixture must produce two disjoint hunks");

    let first_hunk_only = FileDiff {
        hunks: vec![diff.files[0].hunks[0].clone()],
        ..diff.files[0].clone()
    };
    provider
        .stage_hunks(&repo, &[first_hunk_only])
        .expect("staging a single known-good hunk should succeed");

    let staged = provider.diff(&repo, &staged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(staged.files[0].hunks.len(), 1);
    assert!(staged.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "L3-changed"));

    let remaining_unstaged = provider.diff(&repo, &unstaged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(remaining_unstaged.files[0].hunks.len(), 1);
    assert!(remaining_unstaged.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "L17-changed"));
}

#[test]
fn unstage_hunks_unstages_only_the_selected_hunk() {
    let repo_dir = init_repo("unstage-hunk-selected");
    write_file(repo_dir.path(), "a.txt", &twenty_lines());
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", &two_far_apart_edits());
    git(repo_dir.path(), &["add", "a.txt"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let staged = provider.diff(&repo, &staged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(staged.files[0].hunks.len(), 2);

    let first_hunk_only = FileDiff {
        hunks: vec![staged.files[0].hunks[0].clone()],
        ..staged.files[0].clone()
    };
    provider
        .unstage_hunks(&repo, &[first_hunk_only])
        .expect("unstaging a single known-good hunk should succeed");

    let remaining_staged = provider.diff(&repo, &staged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(remaining_staged.files[0].hunks.len(), 1);
    assert!(remaining_staged.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "L17-changed"));

    let unstaged_again = provider.diff(&repo, &unstaged_diff_request(), &CancellationToken::new()).unwrap();
    assert_eq!(unstaged_again.files[0].hunks.len(), 1);
    assert!(unstaged_again.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .any(|l| l.content == "L3-changed"));
}

#[test]
fn stage_hunks_rejects_a_stale_selection_as_operation_conflict() {
    let repo_dir = init_repo("stage-hunk-stale");
    write_file(repo_dir.path(), "a.txt", &twenty_lines());
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", &two_far_apart_edits());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let diff = provider.diff(&repo, &unstaged_diff_request(), &CancellationToken::new()).unwrap();
    let stale_hunk = FileDiff {
        hunks: vec![diff.files[0].hunks[0].clone()],
        ..diff.files[0].clone()
    };

    // Another terminal/session stages a conflicting change to the exact
    // same region before this selection is applied.
    let mut lines: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
    lines[2] = "someone-elses-change".to_string();
    write_file(repo_dir.path(), "a.txt", &(lines.join("\n") + "\n"));
    git(repo_dir.path(), &["add", "a.txt"]);
    // Restore the working tree to what the stale selection still expects,
    // so only the *index* has diverged from the fetched diff.
    write_file(repo_dir.path(), "a.txt", &two_far_apart_edits());

    let err = provider
        .stage_hunks(&repo, &[stale_hunk])
        .expect_err("a hunk whose context no longer matches the index must not apply blindly");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
}

#[test]
fn stage_hunks_rejects_binary_files() {
    let repo_dir = init_repo("stage-hunk-binary-guard");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let binary = FileDiff {
        path: PathBuf::from("image.png"),
        previous_path: None,
        change_type: ChangeType::Modified,
        is_binary: true,
        truncated: false,
        hunks: vec![DiffHunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![DiffLine {
                origin: DiffLineOrigin::Context,
                content: String::new(),
                has_trailing_newline: true,
            }],
        }],
    };

    let err = provider
        .stage_hunks(&repo, &[binary])
        .expect_err("hunk-level staging must reject binary files rather than corrupt them");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

// ---------------------------------------------------------------------
// Switch branch (US-021).
// ---------------------------------------------------------------------

#[test]
fn switch_branch_updates_head_to_the_target_branch() {
    let repo_dir = init_repo("switch-branch-success");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["branch", "feature"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    provider
        .switch_branch(&repo, &BranchName::new("feature").unwrap())
        .expect("switch should succeed with no conflicting local changes");

    let repo_after = provider.discover(repo_dir.path()).unwrap();
    assert_eq!(
        repo_after.current_branch.as_ref().map(BranchName::as_str),
        Some("feature")
    );
}

#[test]
fn switch_branch_refuses_to_discard_conflicting_local_changes() {
    let repo_dir = init_repo("switch-branch-conflict");
    write_file(repo_dir.path(), "a.txt", "content on main\n");
    commit_all(repo_dir.path(), "first commit on main");
    git(repo_dir.path(), &["checkout", "--quiet", "-b", "feature"]);
    write_file(repo_dir.path(), "a.txt", "content on feature\n");
    commit_all(repo_dir.path(), "diverging commit on feature");
    git(repo_dir.path(), &["checkout", "--quiet", "main"]);
    write_file(repo_dir.path(), "a.txt", "uncommitted local edit\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .switch_branch(&repo, &BranchName::new("feature").unwrap())
        .expect_err("switch must refuse rather than discard the uncommitted edit");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    let repo_after = provider.discover(repo_dir.path()).unwrap();
    assert_eq!(
        repo_after.current_branch.as_ref().map(BranchName::as_str),
        Some("main"),
        "a refused switch must leave HEAD on the original branch"
    );
    let content = std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap();
    assert_eq!(
        content, "uncommitted local edit\n",
        "a refused switch must preserve the uncommitted local edit"
    );
}

// ---------------------------------------------------------------------
// Create branch (US-022).
// ---------------------------------------------------------------------

#[test]
fn create_branch_points_at_the_given_start_point_without_switching() {
    let repo_dir = init_repo("create-branch-start-point");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    let commit_a = head_commit_hash(repo_dir.path());
    write_file(repo_dir.path(), "a.txt", "hello again\n");
    commit_all(repo_dir.path(), "second commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    provider
        .create_branch(&repo, &BranchName::new("feature").unwrap(), Some(&commit_a))
        .expect("create should succeed for an explicit, existing start point");

    let repo_after = provider.discover(repo_dir.path()).unwrap();
    assert_eq!(
        repo_after.current_branch.as_ref().map(BranchName::as_str),
        Some("main"),
        "creating a branch must not switch to it"
    );

    let branches = provider.branches(&repo_after).unwrap();
    let feature = branches
        .iter()
        .find(|b| b.name.as_str() == "feature")
        .expect("the new branch should be listed");
    assert_eq!(feature.target, commit_a);
}

#[test]
fn create_branch_rejects_an_existing_name_without_overwriting_it() {
    let repo_dir = init_repo("create-branch-collision");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    let commit_a = head_commit_hash(repo_dir.path());
    git(repo_dir.path(), &["branch", "feature"]);
    write_file(repo_dir.path(), "a.txt", "hello again\n");
    commit_all(repo_dir.path(), "second commit");
    let commit_b = head_commit_hash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .create_branch(&repo, &BranchName::new("feature").unwrap(), Some(&commit_b))
        .expect_err("must refuse to overwrite an existing branch");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    let branches = provider.branches(&repo).unwrap();
    let feature = branches.iter().find(|b| b.name.as_str() == "feature").unwrap();
    assert_eq!(
        feature.target, commit_a,
        "the existing branch must still point at its original commit"
    );
}

// ---------------------------------------------------------------------
// Delete branch (US-023).
// ---------------------------------------------------------------------

#[test]
fn delete_branch_removes_a_merged_local_branch() {
    let repo_dir = init_repo("delete-branch-merged");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["branch", "feature"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    provider
        .delete_branch(&repo, &BranchName::new("feature").unwrap(), false)
        .expect("deleting a fully-merged branch should succeed");

    let branches = provider.branches(&repo).unwrap();
    assert!(!branches.iter().any(|b| b.name.as_str() == "feature"));
}

#[test]
fn delete_branch_refuses_the_current_branch() {
    let repo_dir = init_repo("delete-branch-current");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .delete_branch(&repo, &BranchName::new("main").unwrap(), false)
        .expect_err("must refuse to delete the currently checked out branch");

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    let branches = provider.branches(&repo).unwrap();
    assert!(
        branches.iter().any(|b| b.name.as_str() == "main"),
        "the current branch must not have been removed"
    );
}

#[test]
fn delete_branch_refuses_unmerged_commits_without_force_but_allows_with_force() {
    let repo_dir = init_repo("delete-branch-unmerged");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["checkout", "--quiet", "-b", "feature"]);
    write_file(repo_dir.path(), "a.txt", "unmerged change\n");
    commit_all(repo_dir.path(), "unmerged commit on feature");
    git(repo_dir.path(), &["checkout", "--quiet", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .delete_branch(&repo, &BranchName::new("feature").unwrap(), false)
        .expect_err("must refuse to silently discard unmerged commits");
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    let branches = provider.branches(&repo).unwrap();
    assert!(
        branches.iter().any(|b| b.name.as_str() == "feature"),
        "the unmerged branch must survive a refused delete"
    );

    provider
        .delete_branch(&repo, &BranchName::new("feature").unwrap(), true)
        .expect("force delete should still be possible for an unmerged branch");
    let branches = provider.branches(&repo).unwrap();
    assert!(!branches.iter().any(|b| b.name.as_str() == "feature"));
}
