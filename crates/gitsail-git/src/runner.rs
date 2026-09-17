//! Process execution boundary (SAD §12).
//!
//! [`GitProcessRunner`] spawns `git` (or an explicitly overridden
//! executable) directly via [`std::process::Command`], never through a
//! shell. Sync-vs-async infrastructure strategy is an open architecture
//! decision (SAD §39), so this runner is deliberately synchronous and
//! blocking, relying on `std::process` and cooperative polling only.

use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gitsail_domain::{ErrorCode, GitSailError};

/// Re-exported from `gitsail-domain` so ports (`RepositoryReadPort`) can
/// accept one without `gitsail-application` depending on this adapter (SAD
/// §5). Kept accessible here too since this is where it is actually
/// consumed (`run_process`/`wait_with_timeout`).
pub use gitsail_domain::CancellationToken;

/// Per-stream cap on captured stdout/stderr, so a chatty or malicious
/// process cannot exhaust memory. Output beyond this is discarded (but
/// still drained from the pipe, see [`read_capped`]); callers needing more
/// than this from Git should page the underlying command instead.
const MAX_CAPTURED_STREAM_BYTES: usize = 8 * 1024 * 1024;

/// Polling interval for exit status, timeout and cancellation checks.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Masks `user:password@` credentials embedded in a `scheme://...` argument
/// (e.g. a remote URL) before it is stored anywhere logs or errors might
/// surface it. Arguments that do not look like such a URL pass through
/// unchanged.
pub fn redact_credentials(arg: &str) -> String {
    let Some(scheme_end) = arg.find("://") else {
        return arg.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &arg[authority_start..];
    let Some(at) = rest.find('@') else {
        return arg.to_string();
    };
    if rest[..at].contains('/') {
        // The '@' belongs to the path, not to a credentials segment.
        return arg.to_string();
    }
    format!("{}***@{}", &arg[..authority_start], &rest[at + 1..])
}

fn redact_args(args: &[String]) -> Vec<String> {
    args.iter().map(|arg| redact_credentials(arg)).collect()
}

/// A located `git` executable and the version it reports.
///
/// No minimum version is enforced here: a minimum supported Git version is
/// an open architecture decision (SAD §39). Callers that need a gate can
/// inspect `detected_version` themselves.
#[derive(Debug, Clone)]
pub struct GitExecutable {
    pub path: PathBuf,
    pub detected_version: String,
}

impl GitExecutable {
    /// Locates `git` (or `override_path` when given) and validates it
    /// responds to `--version`, without a shell (SAD §11, §30).
    pub fn discover(override_path: Option<&Path>) -> Result<Self, GitSailError> {
        let path = override_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("git"));

        let output = Command::new(&path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .map_err(|err| {
                GitSailError::new(ErrorCode::GitNotInstalled, "git executable was not found")
                    .with_remediation(
                        "install Git and ensure it is on PATH, or configure an explicit path",
                    )
                    .with_source(err)
            })?;

        if !output.status.success() {
            return Err(GitSailError::new(
                ErrorCode::GitNotInstalled,
                "git executable did not respond successfully to --version",
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let detected_version = parse_git_version(&stdout).ok_or_else(|| {
            GitSailError::new(
                ErrorCode::ParseFailure,
                "could not parse git --version output",
            )
        })?;

        Ok(Self {
            path,
            detected_version,
        })
    }
}

/// Parses the `git version X.Y.Z[...]` line into just the version token.
fn parse_git_version(output: &str) -> Option<String> {
    let rest = output.trim().strip_prefix("git version ")?;
    let version = rest.split_whitespace().next()?;
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

/// A single process invocation. Executable, arguments, environment and
/// working directory are always explicit and passed directly to
/// [`std::process::Command`] — never interpolated into a shell string.
#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub timeout: Option<Duration>,
    /// Bytes written to the child's stdin, then closed so it observes EOF.
    /// `None` (the default) gives the child no stdin at all
    /// ([`Stdio::null`]), matching every read-only invocation.
    pub stdin: Option<Vec<u8>>,
}

impl ProcessRequest {
    pub fn new(args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            args,
            cwd,
            env: Vec::new(),
            timeout: None,
            stdin: None,
        }
    }

    #[must_use]
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    #[must_use]
    pub fn with_stdin(mut self, stdin: Vec<u8>) -> Self {
        self.stdin = Some(stdin);
        self
    }
}

/// Result of a completed (or cut-short) process invocation.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub cancelled: bool,
}

/// Configuration for a [`GitProcessRunner`].
#[derive(Debug, Clone, Default)]
pub struct GitProcessRunnerConfig {
    /// Explicit override for the `git` executable; defaults to `git` on `PATH`.
    pub executable: Option<PathBuf>,
    /// Working directory used by [`GitProcessRunner::build_request`].
    pub default_cwd: Option<PathBuf>,
    /// Timeout applied to requests that do not set their own.
    pub default_timeout: Option<Duration>,
}

/// Owns Git executable discovery and runs Git processes with explicit
/// arguments, environment, timeout and cancellation handling (SAD §12).
pub struct GitProcessRunner {
    executable: GitExecutable,
    default_cwd: Option<PathBuf>,
    default_timeout: Option<Duration>,
}

impl GitProcessRunner {
    pub fn new(config: GitProcessRunnerConfig) -> Result<Self, GitSailError> {
        let executable = GitExecutable::discover(config.executable.as_deref())?;
        Ok(Self {
            executable,
            default_cwd: config.default_cwd,
            default_timeout: config.default_timeout,
        })
    }

    pub fn executable(&self) -> &GitExecutable {
        &self.executable
    }

    /// Convenience constructor for a request rooted at the runner's
    /// configured default working directory.
    pub fn build_request(&self, args: Vec<String>) -> Result<ProcessRequest, GitSailError> {
        let cwd = self.default_cwd.clone().ok_or_else(|| {
            GitSailError::new(
                ErrorCode::Internal,
                "no default working directory configured; build a ProcessRequest directly",
            )
        })?;
        Ok(ProcessRequest {
            args,
            cwd,
            env: Vec::new(),
            timeout: self.default_timeout,
            stdin: None,
        })
    }

    /// Runs `request` against the configured `git` executable, respecting
    /// `cancel` for cooperative cancellation from another thread.
    pub fn run(
        &self,
        mut request: ProcessRequest,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, GitSailError> {
        if request.timeout.is_none() {
            request.timeout = self.default_timeout;
        }
        run_process(&self.executable.path, request, cancel)
    }
}

/// Low-level process execution shared by [`GitProcessRunner::run`]. Exposed
/// so infrastructure and tests can exercise timeout/cancellation behavior
/// against arbitrary executables, not only `git`.
pub fn run_process(
    executable: &Path,
    mut request: ProcessRequest,
    cancel: &CancellationToken,
) -> Result<ProcessOutput, GitSailError> {
    let safe_args = redact_args(&request.args);
    let stdin_bytes = request.stdin.take();

    let mut command = Command::new(executable);
    command
        .args(&request.args)
        .current_dir(&request.cwd)
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The child inherits the parent's environment (PATH, HOME, credential
    // helpers, locale, ...); `env` only adds or overrides specific vars.
    for (key, value) in &request.env {
        command.env(key, value);
    }

    let start = Instant::now();
    let mut child = command.spawn().map_err(|err| {
        GitSailError::new(ErrorCode::ProcessFailure, "failed to spawn git process")
            .with_remediation("verify the configured git executable is accessible")
            .with_source(err)
    })?;

    // Written on its own thread and dropped (closing the pipe, signalling
    // EOF to the child) as soon as the write completes: a child that reads
    // stdin before producing output would otherwise deadlock against a
    // parent still waiting to write everything up front.
    let stdin_handle = stdin_bytes.map(|bytes| {
        let mut stdin_pipe = child.stdin.take().expect("stdin was piped");
        thread::spawn(move || {
            let _ = stdin_pipe.write_all(&bytes);
        })
    });

    let mut stdout_pipe = child.stdout.take().expect("stdout was piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Read both pipes on dedicated threads: a process that fills stdout
    // while we only drain stderr (or vice versa) would otherwise deadlock.
    let stdout_handle =
        thread::spawn(move || read_capped(&mut stdout_pipe, MAX_CAPTURED_STREAM_BYTES));
    let stderr_handle =
        thread::spawn(move || read_capped(&mut stderr_pipe, MAX_CAPTURED_STREAM_BYTES));

    let outcome = wait_with_timeout(&mut child, request.timeout, cancel);
    let duration = start.elapsed();

    // On timeout/cancellation, kill before joining the reader threads: a
    // silent child (e.g. `sleep`) never closes its pipes on its own, so
    // joining first would block the reader threads until natural exit.
    if matches!(outcome, WaitOutcome::TimedOut | WaitOutcome::Cancelled) {
        kill_and_reap(&mut child);
    }

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    if let Some(handle) = stdin_handle {
        let _ = handle.join();
    }

    match outcome {
        WaitOutcome::Exited(status) => {
            if status.success() {
                Ok(ProcessOutput {
                    stdout,
                    stderr,
                    exit_code: status.code(),
                    duration,
                    cancelled: false,
                })
            } else {
                Err(process_failure_error(&safe_args, status.code(), &stderr))
            }
        }
        WaitOutcome::TimedOut => Err(timeout_error(&safe_args, request.timeout, &stderr)),
        WaitOutcome::Cancelled => Err(cancelled_error(&safe_args, &stderr)),
    }
}

enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
    Cancelled,
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Option<Duration>,
    cancel: &CancellationToken,
) -> WaitOutcome {
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return WaitOutcome::Exited(status);
        }
        if cancel.is_cancelled() {
            return WaitOutcome::Cancelled;
        }
        if let Some(timeout) = timeout {
            if start.elapsed() >= timeout {
                return WaitOutcome::TimedOut;
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a pipe to completion, keeping only the first `cap` bytes. Bytes
/// beyond the cap are still drained (and dropped) so a process producing
/// more output than the cap never blocks on a full pipe buffer.
fn read_capped<R: Read>(reader: &mut R, cap: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let take = (cap - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    buf
}

/// Diagnostic detail attached to process-related errors via `with_source`.
/// Never rendered as the user-safe message (SAD §19, §28); arguments are
/// redacted before this is constructed.
#[derive(Debug)]
struct ProcessDiagnostic {
    args: Vec<String>,
    exit_code: Option<i32>,
    stderr: String,
}

impl fmt::Display for ProcessDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exit_code={:?} args={:?} stderr={}",
            self.exit_code, self.args, self.stderr
        )
    }
}

impl std::error::Error for ProcessDiagnostic {}

fn process_failure_error(
    safe_args: &[String],
    exit_code: Option<i32>,
    stderr: &[u8],
) -> GitSailError {
    GitSailError::new(
        ErrorCode::ProcessFailure,
        "git process exited with a non-zero status",
    )
    .with_source(ProcessDiagnostic {
        args: safe_args.to_vec(),
        exit_code,
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    })
}

fn timeout_error(safe_args: &[String], timeout: Option<Duration>, stderr: &[u8]) -> GitSailError {
    GitSailError::new(
        ErrorCode::Timeout,
        format!(
            "git process timed out after {:?}",
            timeout.unwrap_or_default()
        ),
    )
    .with_remediation("retry with a longer timeout or check network/repository state")
    .with_source(ProcessDiagnostic {
        args: safe_args.to_vec(),
        exit_code: None,
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    })
}

fn cancelled_error(safe_args: &[String], stderr: &[u8]) -> GitSailError {
    GitSailError::new(ErrorCode::Cancelled, "git process was cancelled").with_source(
        ProcessDiagnostic {
            args: safe_args.to_vec(),
            exit_code: None,
            stderr: String::from_utf8_lossy(stderr).into_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_git_version_output() {
        assert_eq!(
            parse_git_version("git version 2.43.0\n"),
            Some("2.43.0".to_string())
        );
        assert_eq!(
            parse_git_version("git version 2.39.2 (Apple Git-143)"),
            Some("2.39.2".to_string())
        );
    }

    #[test]
    fn rejects_unrecognized_version_output() {
        assert_eq!(parse_git_version("not git at all"), None);
        assert_eq!(parse_git_version(""), None);
    }

    #[test]
    fn redacts_credentials_from_url_like_arguments() {
        let redacted = redact_credentials("https://user:secret-token@github.com/org/repo.git");
        assert_eq!(redacted, "https://***@github.com/org/repo.git");
        assert!(!redacted.contains("secret-token"));
    }

    #[test]
    fn leaves_non_credential_arguments_untouched() {
        assert_eq!(redact_credentials("--porcelain=v2"), "--porcelain=v2");
        assert_eq!(
            redact_credentials("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
        assert_eq!(
            redact_credentials("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
    }

    #[test]
    fn cancellation_token_reflects_cancel_across_clones() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
