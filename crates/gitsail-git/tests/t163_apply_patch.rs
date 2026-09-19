//! Integration tests for T-163/US-030 — `RepositoryWritePort::preview_patch_application`/
//! `apply_patch` — against a real, temporary Git repository (never a mock),
//! mirroring `tests/provider.rs`'s own convention.
//!
//! The malicious-patch tests here are the empirical verification this
//! story's DoD explicitly requires: a path-traversal patch, an
//! absolute-path patch, and a symlink-escape patch must never write
//! outside the repository and must never corrupt repository state, proven
//! against the real `git` binary rather than assumed from documentation.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::ErrorCode;
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
        let path =
            std::env::temp_dir().join(format!("gitsail-git-apply-patch-{label}-{nanos}-{n}"));
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

/// The exact content `git status --porcelain` reports for `dir`, used to
/// assert a rejected/failed apply left the repository completely
/// untouched (criterion 3: never a silent partial application).
fn status_porcelain(dir: &Path) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .expect("git status must run");
    String::from_utf8(output.stdout).unwrap()
}

// ---------------------------------------------------------------------
// A valid patch: preview reports it supported, apply actually applies it.
// ---------------------------------------------------------------------

#[test]
fn preview_reports_a_valid_patch_as_supported_with_its_affected_file() {
    let repo_dir = init_repo("preview-valid");
    write_file(repo_dir.path(), "a.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let patch =
        "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2-changed\n line3\n";
    let preview = provider.preview_patch_application(&repo, patch).unwrap();

    assert!(preview.supported, "a valid patch must preview as supported");
    assert!(preview.rejection_reason.is_none());
    assert_eq!(preview.affected_files, vec![PathBuf::from("a.txt")]);

    // Building the preview must be a pure read: nothing changed on disk.
    assert_eq!(status_porcelain(repo_dir.path()), "");
}

#[test]
fn apply_patch_applies_a_valid_patch_to_the_working_tree_only() {
    let repo_dir = init_repo("apply-valid");
    write_file(repo_dir.path(), "a.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let patch =
        "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2-changed\n line3\n";
    let result = provider.apply_patch(&repo, patch).unwrap();

    assert_eq!(result.applied_files, vec![PathBuf::from("a.txt")]);
    let on_disk = std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap();
    assert_eq!(on_disk, "line1\nline2-changed\nline3\n");

    // Working tree only: never staged into the index (mirrors a plain
    // `git apply` with no `--cached`/`--index`).
    let status = status_porcelain(repo_dir.path());
    assert!(
        status.starts_with(" M"),
        "expected an unstaged modification, got {status:?}"
    );
}

#[test]
fn apply_patch_can_add_a_new_file() {
    let repo_dir = init_repo("apply-new-file");
    write_file(repo_dir.path(), "a.txt", "line1\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let patch = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\n";
    let result = provider.apply_patch(&repo, patch).unwrap();

    assert_eq!(result.applied_files, vec![PathBuf::from("new.txt")]);
    let contents = std::fs::read_to_string(repo_dir.path().join("new.txt")).unwrap();
    assert_eq!(contents, "hello\nworld\n");
}

// ---------------------------------------------------------------------
// Rejections: malformed patch, stale/incompatible context.
// ---------------------------------------------------------------------

#[test]
fn preview_rejects_a_malformed_patch_without_touching_the_repository() {
    let repo_dir = init_repo("preview-malformed");
    write_file(repo_dir.path(), "a.txt", "line1\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let preview = provider
        .preview_patch_application(&repo, "this is not a patch at all\njust garbage text\n")
        .unwrap();

    assert!(!preview.supported);
    assert!(preview.rejection_reason.is_some());
    assert!(preview.affected_files.is_empty());
    assert_eq!(status_porcelain(repo_dir.path()), "");
}

#[test]
fn apply_patch_rejects_a_malformed_patch_with_a_clear_error_and_no_side_effect() {
    let repo_dir = init_repo("apply-malformed");
    write_file(repo_dir.path(), "a.txt", "line1\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .apply_patch(&repo, "this is not a patch at all\njust garbage text\n")
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::ParseFailure);
    assert!(!err.message().is_empty());
    assert_eq!(status_porcelain(repo_dir.path()), "");
}

#[test]
fn preview_rejects_a_patch_whose_context_no_longer_matches_the_current_file() {
    let repo_dir = init_repo("preview-stale-context");
    write_file(repo_dir.path(), "a.txt", "completely different content\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // This patch's context (`line1`/`line2`/`line3`) does not match the
    // file's real, current content.
    let patch =
        "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2-changed\n line3\n";
    let preview = provider.preview_patch_application(&repo, patch).unwrap();

    assert!(!preview.supported);
    assert_eq!(preview.affected_files, vec![PathBuf::from("a.txt")]);
    let reason = preview.rejection_reason.unwrap();
    assert!(
        reason.contains("no longer applies") || reason.to_lowercase().contains("context"),
        "expected a stale-context reason, got {reason:?}"
    );
}

#[test]
fn apply_patch_rejects_a_stale_context_as_an_operation_conflict_with_no_side_effect() {
    let repo_dir = init_repo("apply-stale-context");
    write_file(repo_dir.path(), "a.txt", "completely different content\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let patch =
        "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2-changed\n line3\n";
    let err = provider.apply_patch(&repo, patch).unwrap_err();

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    assert_eq!(status_porcelain(repo_dir.path()), "");
}

// ---------------------------------------------------------------------
// Malicious patches: path traversal, absolute path, symlink escape.
// Empirical verification (DoD): never written outside the repository,
// never a partial/silent effect on the repository itself.
// ---------------------------------------------------------------------

#[test]
fn apply_patch_rejects_a_path_traversal_patch_and_creates_nothing_outside_the_repository() {
    let repo_dir = init_repo("apply-path-traversal");
    write_file(repo_dir.path(), "a.txt", "line1\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // A canary file outside the repository the traversal patch targets;
    // if this adapter ever let the traversal through, this file would
    // change.
    let outside = std::env::temp_dir().join(format!(
        "gitsail-t163-canary-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&outside, "untouched\n").unwrap();

    // The repo lives directly under a temp dir, so `../../<canary file
    // name>` from the repo root reliably escapes it regardless of the
    // exact nesting depth the OS temp dir happens to have.
    let canary_name = outside.file_name().unwrap().to_str().unwrap();
    let patch = format!(
        "--- /dev/null\n+++ b/../../{canary_name}\n@@ -0,0 +1,1 @@\n+pwned::0:0:pwned:/root:/bin/bash\n"
    );

    let preview = provider.preview_patch_application(&repo, &patch).unwrap();
    assert!(
        !preview.supported,
        "a path-traversal patch must never preview as supported"
    );
    assert_eq!(
        preview.affected_files,
        vec![PathBuf::from(format!("../../{canary_name}"))]
    );
    assert!(preview
        .rejection_reason
        .unwrap()
        .contains("outside the repository"));

    let err = provider.apply_patch(&repo, &patch).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    let canary_contents = std::fs::read_to_string(&outside).unwrap();
    assert_eq!(
        canary_contents, "untouched\n",
        "a path-traversal patch must never write outside the repository"
    );
    assert_eq!(
        status_porcelain(repo_dir.path()),
        "",
        "a rejected malicious patch must never leave a side effect on the repository either"
    );

    std::fs::remove_file(&outside).ok();
}

#[test]
fn apply_patch_rejects_an_absolute_path_patch_and_creates_nothing_outside_the_repository() {
    let repo_dir = init_repo("apply-absolute-path");
    write_file(repo_dir.path(), "a.txt", "line1\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let target = std::env::temp_dir().join(format!(
        "gitsail-t163-abspath-{}.txt",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // Ensure a clean slate: this file must not exist before, and must not
    // exist after the rejected apply either.
    let _ = std::fs::remove_file(&target);

    let patch = format!(
        "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,1 @@\n+pwned\n",
        target.display()
    );

    let preview = provider.preview_patch_application(&repo, &patch).unwrap();
    assert!(
        !preview.supported,
        "an absolute-path patch must never preview as supported"
    );
    assert!(preview
        .rejection_reason
        .unwrap()
        .contains("outside the repository"));

    let err = provider.apply_patch(&repo, &patch).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    assert!(
        !target.exists(),
        "an absolute-path patch must never create a file outside the repository"
    );
    assert_eq!(status_porcelain(repo_dir.path()), "");
}

// Unix-only by nature: the escape it exercises is a real symlink, which
// needs privileges Windows does not grant by default. It used to compile
// everywhere and `panic!` on Windows, which both failed that CI leg and
// left unreachable code after the panic.
#[cfg(unix)]
#[test]
fn apply_patch_rejects_a_symlink_escape_patch_and_creates_nothing_outside_the_repository() {
    let repo_dir = init_repo("apply-symlink-escape");
    write_file(repo_dir.path(), "a.txt", "line1\n");

    let outside_dir = std::env::temp_dir().join(format!(
        "gitsail-t163-symlink-target-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&outside_dir).unwrap();

    std::os::unix::fs::symlink(&outside_dir, repo_dir.path().join("link_to_outside")).unwrap();

    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // No literal `..` and no absolute path here — this patch only escapes
    // the repository by following a symlink the repository itself
    // contains, exercising Git's own "beyond a symbolic link" refusal
    // rather than this adapter's own `..`/absolute-path pre-check.
    let patch =
        "--- /dev/null\n+++ b/link_to_outside/pwned_via_symlink.txt\n@@ -0,0 +1,1 @@\n+pwned\n";

    let preview = provider.preview_patch_application(&repo, patch).unwrap();
    assert!(
        !preview.supported,
        "a symlink-escape patch must never preview as supported"
    );

    let err = provider.apply_patch(&repo, patch).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    assert!(
        !outside_dir.join("pwned_via_symlink.txt").exists(),
        "a symlink-escape patch must never create a file outside the repository"
    );
    assert_eq!(status_porcelain(repo_dir.path()), "");

    std::fs::remove_dir_all(&outside_dir).ok();
}

// ---------------------------------------------------------------------
// Atomicity: a multi-file patch failing on one file applies none of them.
// ---------------------------------------------------------------------

#[test]
fn apply_patch_is_all_or_nothing_across_files() {
    let repo_dir = init_repo("apply-mixed-atomic");
    write_file(repo_dir.path(), "a.txt", "line1\nline2\nline3\n");
    write_file(repo_dir.path(), "b.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "init");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // a.txt's hunk is valid; b.txt's hunk has a deliberately wrong context
    // line, so the whole patch must fail — including for a.txt, which
    // would otherwise have applied cleanly on its own.
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+line2-changed\n line3\n--- a/b.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n WRONG_CONTEXT_LINE\n-line2\n+line2-changed-b\n line3\n";

    let err = provider.apply_patch(&repo, patch).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    assert_eq!(
        status_porcelain(repo_dir.path()),
        "",
        "a.txt must not have been partially applied while b.txt failed"
    );
    let a_contents = std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap();
    assert_eq!(a_contents, "line1\nline2\nline3\n");
}

// ---------------------------------------------------------------------
// Bare repository: no working tree to apply into.
// ---------------------------------------------------------------------

#[test]
fn preview_and_apply_refuse_a_bare_repository_with_no_working_tree() {
    let repo_dir = TempDir::new("apply-bare");
    git(repo_dir.path(), &["init", "--quiet", "--bare"]);
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";

    let preview_err = provider
        .preview_patch_application(&repo, patch)
        .unwrap_err();
    assert_eq!(preview_err.code(), ErrorCode::InvalidRepositoryState);

    let apply_err = provider.apply_patch(&repo, patch).unwrap_err();
    assert_eq!(apply_err.code(), ErrorCode::InvalidRepositoryState);
}
