//! Integration tests for the process execution boundary (SAD §12).
//!
//! Covers success, failure, timeout and cancellation. `git` itself is only
//! used for the success/failure cases; timeout/cancellation use a plain
//! long-running command so the test does not depend on any particular Git
//! behavior sleeping.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gitsail_domain::ErrorCode;
use gitsail_git::{
    run_process, CancellationToken, GitProcessRunner, GitProcessRunnerConfig, ProcessRequest,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-test-{label}-{nanos}-{n}"));
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

fn runner() -> GitProcessRunner {
    GitProcessRunner::new(GitProcessRunnerConfig::default()).expect("git must be installed")
}

#[test]
fn runs_git_version_successfully() {
    let runner = runner();
    let cwd = std::env::current_dir().unwrap();
    let request = ProcessRequest::new(vec!["--version".to_string()], cwd);

    let output = runner
        .run(request, &CancellationToken::new())
        .expect("git --version should succeed");

    assert_eq!(output.exit_code, Some(0));
    assert!(!output.cancelled);
    assert!(String::from_utf8_lossy(&output.stdout).contains("git version"));
}

#[test]
fn reports_process_failure_for_non_zero_exit() {
    let runner = runner();
    let repo = TempDir::new("failure-repo");

    let init = ProcessRequest::new(
        vec!["init".to_string(), "--quiet".to_string()],
        repo.path().to_path_buf(),
    );
    runner
        .run(init, &CancellationToken::new())
        .expect("git init should succeed");

    let request = ProcessRequest::new(
        vec![
            "rev-parse".to_string(),
            "--verify".to_string(),
            "refs/heads/does-not-exist".to_string(),
        ],
        repo.path().to_path_buf(),
    );

    let err = runner
        .run(request, &CancellationToken::new())
        .expect_err("verifying a missing ref should fail");

    assert_eq!(err.code(), ErrorCode::ProcessFailure);
    assert!(err.diagnostic().is_some());
    // The safe message must never carry raw stderr/ref content.
    assert!(!err.message().contains("does-not-exist"));
}

#[cfg(unix)]
fn sleep_command(seconds: &str) -> (PathBuf, Vec<String>) {
    (PathBuf::from("sleep"), vec![seconds.to_string()])
}

#[cfg(windows)]
fn sleep_command(seconds: &str) -> (PathBuf, Vec<String>) {
    (
        PathBuf::from("timeout"),
        vec![
            "/T".to_string(),
            seconds.to_string(),
            "/NOBREAK".to_string(),
        ],
    )
}

#[test]
fn times_out_a_long_running_process() {
    let (executable, args) = sleep_command("2");
    let cwd = std::env::current_dir().unwrap();
    let request = ProcessRequest::new(args, cwd).with_timeout(Duration::from_millis(150));

    let err = run_process(&executable, request, &CancellationToken::new())
        .expect_err("a 2s sleep with a 150ms timeout should time out");

    assert_eq!(err.code(), ErrorCode::Timeout);
}

#[test]
fn cancellation_stops_a_running_process() {
    let (executable, args) = sleep_command("5");
    let cwd = std::env::current_dir().unwrap();
    let request = ProcessRequest::new(args, cwd);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        cancel_clone.cancel();
    });

    let start = std::time::Instant::now();
    let err = run_process(&executable, request, &cancel).expect_err("cancelled process errors");
    let elapsed = start.elapsed();

    canceller.join().unwrap();

    assert_eq!(err.code(), ErrorCode::Cancelled);
    // The process must actually have been killed, not run to completion.
    assert!(elapsed < Duration::from_secs(3), "elapsed was {elapsed:?}");
}
