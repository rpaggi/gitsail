//! Integration tests for the process execution boundary (SAD §12).
//!
//! Covers success, failure, timeout and cancellation. `git` itself is only
//! used for the success/failure cases; timeout/cancellation use a plain
//! long-running command so the test does not depend on any particular Git
//! behavior sleeping.

use std::path::PathBuf;
// Only the `cfg(unix)` helpers below take a `&Path`, so at unconditional
// module scope this is a dead import on Windows.
#[cfg(unix)]
use std::path::Path;
use std::time::Duration;

use gitsail_domain::ErrorCode;
use gitsail_git::{
    run_process, CancellationToken, GitProcessRunner, GitProcessRunnerConfig, ProcessRequest,
};
// T-252/US-119: `TempDir` used to be duplicated here (and in ~9 other
// integration test files); it now lives in `gitsail-test-support`, this
// crate's own test-only fixture crate (see that crate's `src/lib.rs` doc).
use gitsail_test_support::TempDir;

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

#[test]
fn stdin_bytes_reach_the_child_process() {
    let runner = runner();
    let repo = TempDir::new("stdin-repo");
    runner
        .run(
            ProcessRequest::new(
                vec!["init".to_string(), "--quiet".to_string()],
                repo.path().to_path_buf(),
            ),
            &CancellationToken::new(),
        )
        .expect("git init should succeed");

    // `git hash-object --stdin` reads its blob content from stdin and
    // echoes back its object id; asserting against the SHA-1 of the exact
    // fixture bytes (computed independently with `git hash-object`) proves
    // those bytes actually reached the child, not merely that the call
    // succeeded with *some* stdin.
    let request = ProcessRequest::new(
        vec!["hash-object".to_string(), "--stdin".to_string()],
        repo.path().to_path_buf(),
    )
    .with_stdin(b"gitsail stdin plumbing\n".to_vec());

    let output = runner
        .run(request, &CancellationToken::new())
        .expect("git hash-object --stdin should succeed");

    let hash = String::from_utf8(output.stdout).unwrap();
    assert_eq!(hash.trim(), "8e3f5c9520f2446ff95e42efd877aab7068c2dbf");
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

// ---------------------------------------------------------------------
// EPIC-22/T-223/US-112: Git credential delegation. GitSail never owns a
// credential store — it must work through whatever `credential.helper`
// (or SSH agent, or OS keychain integration Git itself already knows about)
// the person already has configured, purely by inheriting the parent
// environment and never overriding `GIT_TERMINAL_PROMPT` (see
// `GitCliProvider::locale_env`, which only ever sets `LC_ALL`/`LANG`).
//
// Unix-only: the fake credential helper is a `#!/bin/sh` script, mirroring
// this file's own `sleep_command` split for OS-specific fixtures, but a
// shell-script "external program" helper is simplest to author only for
// Unix; Windows CI exercises the same `GitProcessRunner` machinery through
// every other test in this file (including the generic timeout/cancel
// tests below, which are cross-platform).
// ---------------------------------------------------------------------

#[cfg(unix)]
fn write_executable_script(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).expect("write fake credential helper script");
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(path, perms).unwrap();
}

#[cfg(unix)]
fn init_repo_with_credential_helper(label: &str, helper_path: &Path) -> TempDir {
    let repo = TempDir::new(label);
    let runner = runner();
    runner
        .run(
            ProcessRequest::new(
                vec!["init".to_string(), "--quiet".to_string()],
                repo.path().to_path_buf(),
            ),
            &CancellationToken::new(),
        )
        .expect("git init should succeed");
    runner
        .run(
            ProcessRequest::new(
                vec![
                    "config".to_string(),
                    "credential.helper".to_string(),
                    helper_path.to_string_lossy().into_owned(),
                ],
                repo.path().to_path_buf(),
            ),
            &CancellationToken::new(),
        )
        .expect("configuring the fake credential helper should succeed");
    repo
}

/// T-223 criteria 1/2: a credential helper already configured by the
/// person (never a GitSail-owned store) supplies credentials through
/// GitSail's own process boundary, exercised via the real `git credential
/// fill` plumbing command so this is not a GitSail-specific code path.
#[cfg(unix)]
#[test]
fn a_preconfigured_credential_helper_supplies_credentials_through_gitsail_process_boundary() {
    let fixtures = TempDir::new("credential-helper-fixtures");
    let helper_path = fixtures.path().join("fake-helper.sh");
    write_executable_script(
        &helper_path,
        "#!/bin/sh\n\
         action=\"$1\"\n\
         if [ \"$action\" = \"get\" ]; then\n\
         echo username=gitsail-test-user\n\
         echo password=SENTINEL_FAKE_TOKEN_9f3c7a\n\
         fi\n",
    );
    let repo = init_repo_with_credential_helper("credential-helper-success", &helper_path);

    let request = ProcessRequest::new(
        vec!["credential".to_string(), "fill".to_string()],
        repo.path().to_path_buf(),
    )
    .with_stdin(b"protocol=https\nhost=example.com\n\n".to_vec());

    let output = runner()
        .run(request, &CancellationToken::new())
        .expect("git credential fill through the configured helper should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("gitsail-test-user"));
}

/// T-223 criterion 3: a credential helper that would otherwise hang
/// forever (e.g. one that opens an interactive prompt with no terminal to
/// answer it) must not hang GitSail indefinitely — a configured timeout
/// aborts it and reports a clear, classified [`ErrorCode::Timeout`], never a
/// silent freeze.
#[cfg(unix)]
#[test]
fn a_hanging_credential_helper_times_out_instead_of_blocking_forever() {
    let fixtures = TempDir::new("credential-helper-hang-fixtures");
    let helper_path = fixtures.path().join("hanging-helper.sh");
    write_executable_script(&helper_path, "#!/bin/sh\nsleep 5\n");
    let repo = init_repo_with_credential_helper("credential-helper-hang", &helper_path);

    let request = ProcessRequest::new(
        vec!["credential".to_string(), "fill".to_string()],
        repo.path().to_path_buf(),
    )
    .with_stdin(b"protocol=https\nhost=example.com\n\n".to_vec())
    .with_timeout(Duration::from_millis(200));

    let start = std::time::Instant::now();
    let err = runner()
        .run(request, &CancellationToken::new())
        .expect_err("a hanging credential helper must time out, not hang forever");
    let elapsed = start.elapsed();

    assert_eq!(err.code(), ErrorCode::Timeout);
    assert!(elapsed < Duration::from_secs(3), "elapsed was {elapsed:?}");
}

// ---------------------------------------------------------------------
// EPIC-22/T-224/US-113: secrets embedded in a process's own stderr (not
// just in the argument list) must never survive into a propagated error's
// diagnostic.
// ---------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_credential_bearing_url_printed_to_stderr_is_redacted_in_the_propagated_error() {
    const SENTINEL: &str = "sentinel-fake-token-9f3c7a";
    let script = format!(
        "#!/bin/sh\necho \"fatal: could not read Username for 'https://user:{SENTINEL}@example.com/repo.git'\" >&2\nexit 1\n"
    );
    let fixtures = TempDir::new("stderr-redaction-fixtures");
    let script_path = fixtures.path().join("fail-with-secret.sh");
    write_executable_script(&script_path, &script);

    let cwd = std::env::current_dir().unwrap();
    let request = ProcessRequest::new(Vec::new(), cwd);

    let err = run_process(&script_path, request, &CancellationToken::new())
        .expect_err("the script exits non-zero");

    assert_eq!(err.code(), ErrorCode::ProcessFailure);
    let diagnostic = err
        .diagnostic()
        .expect("a process failure always carries a diagnostic")
        .to_string();
    assert!(
        !diagnostic.contains(SENTINEL),
        "diagnostic must never contain the credential: {diagnostic}"
    );
    // The rest of the message remains useful for troubleshooting.
    assert!(diagnostic.contains("could not read Username"));
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
