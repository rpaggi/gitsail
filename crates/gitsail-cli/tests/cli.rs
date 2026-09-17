//! Integration/smoke tests for the `gitsail` binary (US-036 DoD: "Smoke
//! tests executam todos os comandos nas fixtures sem alterar o
//! repositório"; US-037 DoD: process test over stdout/exit codes; US-038
//! DoD: cancellation/timeout test).
//!
//! Every test spawns the real compiled binary (`env!("CARGO_BIN_EXE_gitsail")`)
//! against a temporary, real Git repository created via the `git` CLI —
//! never a mock — following the same fixture convention as
//! `gitsail-git`'s own integration tests.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
    assert_eq!(before, after, "read-only commands must never change repository state");
}

#[test]
fn open_reports_the_discovered_repository() {
    let repo = sample_repo();
    let output = run(&["open", "--repo", repo.path().to_str().unwrap()]);

    assert!(output.status.success());
    let stdout = stdout_of(&output);
    assert!(stdout.contains("main"), "expected the current branch in output: {stdout}");
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
    assert_eq!(lines.len(), 1, "stdout must carry exactly one JSON line, got: {stdout:?}");

    let json: serde_json::Value = serde_json::from_str(lines[0]).expect("stdout must be valid JSON");
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
    assert_eq!(output.status.code(), Some(3), "RepositoryNotFound must map to a distinct exit code");
    let stdout = stdout_of(&output);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout must be valid JSON even on error");
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
    assert!(stdout_of(&output).is_empty(), "an error must never be written to stdout in human mode");
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

fn fake_git_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-git")
}

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
    assert_eq!(output.status.code(), Some(8), "Timeout must map to its own exit code");
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout must still be a valid envelope on timeout");
    assert_eq!(json["error"]["code"], "timeout");
}

#[cfg(unix)]
#[test]
fn ctrl_c_cancels_an_in_flight_diff_and_kills_the_child_process() {
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
    assert_eq!(output.status.code(), Some(130), "Cancelled must exit 130 (128 + SIGINT)");

    let mut stdout = String::new();
    let _ = std::io::Cursor::new(&output.stdout).read_to_string(&mut stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("a cancelled command must still print a valid envelope");
    assert_eq!(json["error"]["code"], "cancelled");
}
