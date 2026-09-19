//! Integration/smoke tests for the `gitsail` binary (US-036 DoD: "Smoke
//! tests executam todos os comandos nas fixtures sem alterar o
//! repositório"; US-037 DoD: process test over stdout/exit codes; US-038
//! DoD: cancellation/timeout test).
//!
//! Every test spawns the real compiled binary (`env!("CARGO_BIN_EXE_gitsail")`)
//! against a temporary, real Git repository created via the `git` CLI —
//! never a mock — following the same fixture convention as
//! `gitsail-git`'s own integration tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
// Only the `cfg(unix)` timeout/cancellation tests measure elapsed time.
#[cfg(unix)]
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-cli-test-{label}-{nanos}-{n}"));
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

fn porcelain_status(dir: &Path) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain=v2"])
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .expect("git status should run");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gitsail"))
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run gitsail {args:?}: {e}"))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn rev_parse(dir: &Path, revision: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", revision])
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .expect("git rev-parse should run");
    assert!(
        output.status.success(),
        "git rev-parse {revision} failed in {dir:?}"
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn json_data(output: &Output) -> serde_json::Value {
    let stdout = stdout_of(output);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{stdout}"));
    assert_eq!(json["status"], "ok", "command did not succeed: {json}");
    json["data"].clone()
}

/// A repository with one commit, one branch, a staged addition and an
/// unstaged modification, so every one of the six commands has something
/// non-trivial to report.
fn sample_repo() -> TempDir {
    let dir = init_repo("sample");
    write_file(dir.path(), "README.md", "hello\n");
    commit_all(dir.path(), "initial commit");
    write_file(dir.path(), "staged.txt", "staged\n");
    git(dir.path(), &["add", "staged.txt"]);
    write_file(dir.path(), "README.md", "hello\nworld\n");
    dir
}

// ---------------------------------------------------------------------
// US-036: six commands, human mode, no repository mutation.
// ---------------------------------------------------------------------

#[test]
fn all_six_commands_succeed_in_human_mode_without_mutating_the_repository() {
    let repo = sample_repo();
    let repo_str = repo.path().to_str().unwrap();
    let before = porcelain_status(repo.path());

    let commands: &[&[&str]] = &[
        &["open", "--repo", repo_str],
        &["status", "--repo", repo_str],
        &["log", "--repo", repo_str],
        &["branches", "--repo", repo_str],
        &["diff", "--repo", repo_str],
        &["blame", "README.md", "--repo", repo_str],
    ];

    for args in commands {
        let output = run(args);
        assert!(
            output.status.success(),
            "gitsail {args:?} failed: stderr={}",
            stderr_of(&output)
        );
        assert!(
            !stdout_of(&output).is_empty(),
            "gitsail {args:?} produced no output"
        );
    }

    let after = porcelain_status(repo.path());
    assert_eq!(
        before, after,
        "read-only commands must never change repository state"
    );
}

#[test]
fn open_reports_the_discovered_repository() {
    let repo = sample_repo();
    let output = run(&["open", "--repo", repo.path().to_str().unwrap()]);

    assert!(output.status.success());
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("main"),
        "expected the current branch in output: {stdout}"
    );
}

#[test]
fn status_reports_staged_and_unstaged_changes() {
    let repo = sample_repo();
    let output = run(&["status", "--repo", repo.path().to_str().unwrap()]);

    assert!(output.status.success());
    let stdout = stdout_of(&output);
    assert!(stdout.contains("staged.txt"));
    assert!(stdout.contains("README.md"));
}

#[test]
fn log_help_documents_options_and_examples() {
    let output = run(&["log", "--help"]);
    assert!(output.status.success());
    let stdout = stdout_of(&output);
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--author"));
    assert!(stdout.contains("Examples"));
}

// ---------------------------------------------------------------------
// US-037: --json mode.
// ---------------------------------------------------------------------

#[test]
fn json_mode_prints_exactly_one_valid_versioned_envelope_on_stdout() {
    let repo = sample_repo();
    let output = run(&["status", "--repo", repo.path().to_str().unwrap(), "--json"]);

    assert!(output.status.success());
    let stdout = stdout_of(&output);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "stdout must carry exactly one JSON line, got: {stdout:?}"
    );

    let json: serde_json::Value =
        serde_json::from_str(lines[0]).expect("stdout must be valid JSON");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["status"], "ok");
    assert!(json["requestId"].is_string());
    assert!(json["data"]["isClean"].is_boolean());
    assert!(stderr_of(&output).is_empty());
}

#[test]
fn json_mode_error_envelope_is_consistent_for_an_invalid_repository() {
    let dir = TempDir::new("not-a-repo");
    let output = run(&["status", "--repo", dir.path().to_str().unwrap(), "--json"]);

    assert!(!output.status.success());
    assert_eq!(
        output.status.code(),
        Some(3),
        "RepositoryNotFound must map to a distinct exit code"
    );
    let stdout = stdout_of(&output);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be valid JSON even on error");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "repository_not_found");
    assert!(json.get("data").is_none());
}

#[test]
fn human_mode_error_goes_to_stderr_not_stdout() {
    let dir = TempDir::new("not-a-repo-human");
    let output = run(&["status", "--repo", dir.path().to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(
        stdout_of(&output).is_empty(),
        "an error must never be written to stdout in human mode"
    );
    assert!(stderr_of(&output).contains("not a Git repository"));
}

// ---------------------------------------------------------------------
// US-036 criterion 3 / usage errors.
// ---------------------------------------------------------------------

#[test]
fn missing_required_argument_is_a_usage_error_with_exit_code_two() {
    let output = run(&["blame"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!stderr_of(&output).is_empty());
}

#[test]
fn unknown_subcommand_is_a_usage_error_with_exit_code_two() {
    let output = run(&["not-a-real-command"]);
    assert_eq!(output.status.code(), Some(2));
}

// ---------------------------------------------------------------------
// US-038: timeout and cooperative Ctrl+C cancellation, both terminating
// the child process rather than hanging.
// ---------------------------------------------------------------------

// `tests/fixtures/fake-git` is a `#!/bin/sh` script, so the two tests that
// stand a slow Git up against it are Unix-only. On Windows it is not
// executable at all and the run fails as a spawn error (exit 4) long
// before any timeout could fire, which is why this used to redden that CI
// leg. Known gap: timeout/cancellation therefore has no end-to-end
// coverage on Windows — closing it needs a `.cmd` equivalent of the
// fixture, not a `cfg` change here.
#[cfg(unix)]
fn fake_git_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-git")
}

#[cfg(unix)]
#[test]
fn a_short_timeout_terminates_a_slow_git_invocation_with_a_distinct_exit_code() {
    let dir = TempDir::new("timeout");
    let start = Instant::now();

    let output = run(&[
        "status",
        "--repo",
        dir.path().to_str().unwrap(),
        "--git-path",
        fake_git_path().to_str().unwrap(),
        "--timeout",
        "0.05",
        "--json",
    ]);

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "a timed-out invocation must not wait for the slow child to finish naturally; elapsed={elapsed:?}"
    );
    assert_eq!(
        output.status.code(),
        Some(8),
        "Timeout must map to its own exit code"
    );
    let json: serde_json::Value = serde_json::from_str(stdout_of(&output).trim())
        .expect("stdout must still be a valid envelope on timeout");
    assert_eq!(json["error"]["code"], "timeout");
}

#[cfg(unix)]
#[test]
fn ctrl_c_cancels_an_in_flight_diff_and_kills_the_child_process() {
    // Scoped to this test rather than the module: it is the only user of
    // either, and at module scope both are dead imports on Windows, where
    // this `cfg(unix)` test does not compile — which fails `clippy -D
    // warnings` on that CI leg alone.
    use std::io::Read;
    use std::process::Stdio;

    let dir = TempDir::new("cancel");
    let child = Command::new(bin())
        .args([
            "diff",
            "--repo",
            dir.path().to_str().unwrap(),
            "--git-path",
            fake_git_path().to_str().unwrap(),
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn gitsail");

    // Give the process time to pass repository discovery (instant, per the
    // fake git script) and reach the actual (slow) diff invocation.
    std::thread::sleep(Duration::from_millis(300));

    let start = Instant::now();
    let sent = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT to gitsail");
    assert!(sent.success(), "sending SIGINT must succeed");

    let output = child.wait_with_output().expect("wait for gitsail to exit");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "cancellation must kill the child rather than waiting out its full sleep; elapsed={elapsed:?}"
    );
    assert_eq!(
        output.status.code(),
        Some(130),
        "Cancelled must exit 130 (128 + SIGINT)"
    );

    let mut stdout = String::new();
    let _ = std::io::Cursor::new(&output.stdout).read_to_string(&mut stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .expect("a cancelled command must still print a valid envelope");
    assert_eq!(json["error"]["code"], "cancelled");
}

// ---------------------------------------------------------------------
// EPIC-15/US-076: `commit`, `commit-diff`, `line-history`, `show-file`.
// ---------------------------------------------------------------------

#[test]
fn commit_reports_the_right_subject_and_author() {
    let repo = sample_repo();
    let repo_str = repo.path().to_str().unwrap();
    let head = rev_parse(repo.path(), "HEAD");

    let output = run(&["commit", &head, "--repo", repo_str, "--json"]);

    assert!(output.status.success(), "stderr={}", stderr_of(&output));
    let data = json_data(&output);
    assert_eq!(data["hash"], head);
    assert_eq!(data["subject"], "initial commit");
    assert_eq!(data["author"]["name"], "Test User");
    assert_eq!(data["author"]["email"], "test@example.com");
}

#[test]
fn commit_diff_on_a_merge_commit_reports_base_as_the_first_parent() {
    let repo = init_repo("commit-diff-merge");
    let repo_str = repo.path().to_str().unwrap();
    write_file(repo.path(), "base.txt", "base\n");
    commit_all(repo.path(), "base commit");

    git(repo.path(), &["checkout", "-b", "feature"]);
    write_file(repo.path(), "feature.txt", "feature\n");
    commit_all(repo.path(), "feature commit");

    git(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "main-side.txt", "main side\n");
    commit_all(repo.path(), "main-side commit");
    // `main`'s tip right before the merge is what `git merge`'s first
    // parent must be: merging into `main` always makes the checked-out
    // branch's current commit the first parent, regardless of when
    // `feature` branched off.
    let first_parent = rev_parse(repo.path(), "main");

    git(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge feature", "feature"],
    );
    let merge_hash = rev_parse(repo.path(), "HEAD");

    let output = run(&["commit-diff", &merge_hash, "--repo", repo_str, "--json"]);

    assert!(output.status.success(), "stderr={}", stderr_of(&output));
    let data = json_data(&output);
    assert_eq!(data["target"], merge_hash);
    assert_eq!(
        data["base"], first_parent,
        "a merge commit's diff base must be its first parent"
    );
}

#[test]
fn line_history_returns_at_least_one_entry_for_a_range_with_history() {
    let repo = init_repo("line-history-cli");
    let repo_str = repo.path().to_str().unwrap();
    write_file(repo.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo.path(), "introduce");
    write_file(repo.path(), "f.txt", "line1\nline2-changed\nline3\n");
    commit_all(repo.path(), "modify line2");

    let output = run(&[
        "line-history",
        "f.txt",
        "--range",
        "2-2",
        "--repo",
        repo_str,
        "--json",
    ]);

    assert!(output.status.success(), "stderr={}", stderr_of(&output));
    let data = json_data(&output);
    assert_eq!(data["file"], "f.txt");
    assert!(
        !data["entries"].as_array().unwrap().is_empty(),
        "expected at least one history entry, got: {data}"
    );
}

#[test]
fn show_file_returns_text_content_for_an_existing_revision() {
    let repo = sample_repo();
    let repo_str = repo.path().to_str().unwrap();
    let head = rev_parse(repo.path(), "HEAD");

    let output = run(&[
        "show-file",
        "README.md",
        "--revision",
        &head,
        "--repo",
        repo_str,
        "--json",
    ]);

    assert!(output.status.success(), "stderr={}", stderr_of(&output));
    let data = json_data(&output);
    assert_eq!(data["kind"], "text");
    assert_eq!(data["path"], "README.md");
    assert_eq!(data["revision"], head);
    assert_eq!(data["content"], "hello\n");
}

#[test]
fn show_file_returns_missing_for_a_path_added_only_after_the_queried_revision() {
    let repo = init_repo("show-file-missing");
    let repo_str = repo.path().to_str().unwrap();
    write_file(repo.path(), "a.txt", "line1\n");
    commit_all(repo.path(), "first commit");
    let first = rev_parse(repo.path(), "HEAD");

    write_file(repo.path(), "b.txt", "added later\n");
    commit_all(repo.path(), "add b.txt");

    let output = run(&[
        "show-file",
        "b.txt",
        "--revision",
        &first,
        "--repo",
        repo_str,
        "--json",
    ]);

    assert!(output.status.success(), "stderr={}", stderr_of(&output));
    let data = json_data(&output);
    assert_eq!(data["kind"], "missing");
    assert_eq!(data["path"], "b.txt");
    assert_eq!(data["revision"], first);
    assert!(data.get("content").is_none());
}
