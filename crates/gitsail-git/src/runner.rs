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
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gitsail_domain::redact::{redact_credential_url, redact_secrets};
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
///
/// Thin wrapper over `gitsail_domain::redact::redact_credential_url`
/// (EPIC-22/T-224/US-113): the canonical implementation moved to
/// `gitsail-domain` so it is centralized and reusable from anywhere in the
/// workspace (not duplicated here and in a future diagnostic-export path),
/// while this name/signature stays stable for existing callers
/// (`gitsail-cli`'s `eprint_debug`, this module's own callers below).
pub fn redact_credentials(arg: &str) -> String {
    redact_credential_url(arg)
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

        let mut probe = Command::new(&path);
        probe.arg("--version").stdin(Stdio::null());
        suppress_console_window(&mut probe);
        let output = probe.output().map_err(|err| {
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
    suppress_console_window(&mut command);
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
    // Results travel back over a channel (rather than a plain `JoinHandle`)
    // so the timeout/cancellation path below can bound how long it waits
    // for them (EPIC-22/T-223/US-112): `git` itself may spawn a credential
    // helper, an askpass prompt, or another grandchild that inherits the
    // piped stdout/stderr file descriptors. Killing only the direct child
    // (`kill_and_reap`, below) does not guarantee such a grandchild is gone
    // too — process-group-wide termination is inherently OS/environment
    // dependent — so an unbounded `.join()` here could still block past any
    // configured timeout on a lingering descendant. Losing at most the
    // partial output of an already-killed, already-timed-out invocation is
    // an acceptable trade for never hanging indefinitely.
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let stdout_thread = thread::spawn(move || {
        let _ = stdout_tx.send(read_capped(&mut stdout_pipe, MAX_CAPTURED_STREAM_BYTES));
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    let stderr_thread = thread::spawn(move || {
        let _ = stderr_tx.send(read_capped(&mut stderr_pipe, MAX_CAPTURED_STREAM_BYTES));
    });

    let outcome = wait_with_timeout(&mut child, request.timeout, cancel);
    let duration = start.elapsed();

    // On timeout/cancellation, kill before waiting on the reader threads: a
    // silent child (e.g. `sleep`) never closes its pipes on its own, so
    // waiting first would block the reader threads until natural exit.
    let read_grace = if matches!(outcome, WaitOutcome::TimedOut | WaitOutcome::Cancelled) {
        kill_and_reap(&mut child);
        Some(READER_DRAIN_GRACE)
    } else {
        None
    };

    let stdout = recv_reader_result(stdout_rx, read_grace);
    let stderr = recv_reader_result(stderr_rx, read_grace);
    // The reader threads themselves are deliberately not joined: on the
    // (rare) grace-period-expired path, one may still be blocked reading a
    // lingering descendant's pipe. Detaching it here — it exits on its own
    // whenever that descendant eventually closes the pipe — is what keeps
    // this function itself from ever blocking past `read_grace`.
    let _ = stdout_thread;
    let _ = stderr_thread;
    if let Some(handle) = stdin_handle {
        let _ = handle.join();
    }

    // Structured local logging only (SAD §28, EPIC-22/T-224/US-113): level,
    // component, operation (the git subcommand, already redacted), duration
    // and outcome/error code — never raw stdout/stderr or unredacted
    // arguments. `operation` is just the subcommand name (e.g. "status",
    // "log"), not the full, potentially path-bearing argument list.
    let operation = safe_args.first().map(String::as_str).unwrap_or("git");
    let duration_ms = duration.as_millis();

    match outcome {
        WaitOutcome::Exited(status) => {
            if status.success() {
                tracing::debug!(
                    component = "gitsail-git",
                    operation,
                    duration_ms,
                    exit_code = status.code(),
                    "git process completed"
                );
                Ok(ProcessOutput {
                    stdout,
                    stderr,
                    exit_code: status.code(),
                    duration,
                    cancelled: false,
                })
            } else {
                tracing::warn!(
                    component = "gitsail-git",
                    operation,
                    duration_ms,
                    exit_code = status.code(),
                    error_code = ErrorCode::ProcessFailure.as_str(),
                    "git process failed"
                );
                Err(process_failure_error(
                    &safe_args,
                    status.code(),
                    &stdout,
                    &stderr,
                ))
            }
        }
        WaitOutcome::TimedOut => {
            tracing::warn!(
                component = "gitsail-git",
                operation,
                duration_ms,
                error_code = ErrorCode::Timeout.as_str(),
                "git process timed out"
            );
            Err(timeout_error(&safe_args, request.timeout, &stderr))
        }
        WaitOutcome::Cancelled => {
            tracing::debug!(
                component = "gitsail-git",
                operation,
                duration_ms,
                error_code = ErrorCode::Cancelled.as_str(),
                "git process cancelled"
            );
            Err(cancelled_error(&safe_args, &stderr))
        }
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

/// How long [`run_process`] waits for the stdout/stderr reader threads to
/// finish after a timeout/cancellation before giving up on them (see
/// [`recv_reader_result`]).
const READER_DRAIN_GRACE: Duration = Duration::from_millis(500);

fn kill_and_reap(child: &mut Child) {
    // Kills the direct child only. A grandchild the child itself spawned
    // (e.g. `git` invoking a credential helper) may keep running and keep
    // the piped stdout/stderr open regardless — reliably reaching an
    // entire process subtree from here is OS/environment dependent (process
    // groups exist on Unix but not the same way on Windows, and are not
    // always honored identically by every process-launching environment).
    // [`READER_DRAIN_GRACE`] is what actually guarantees `run_process` never
    // hangs on such a survivor, not this call.
    let _ = child.kill();
    let _ = child.wait();
}

/// Receives a reader thread's result, either unbounded (`grace: None`, the
/// normal-exit path, where the child is expected to have already closed its
/// pipes) or bounded by `grace` (the timeout/cancellation path, where a
/// surviving grandchild could otherwise hold the pipe open indefinitely).
/// Times out to an empty `Vec` rather than blocking — losing at most a
/// killed process's trailing output, never the caller's own responsiveness.
fn recv_reader_result(rx: mpsc::Receiver<Vec<u8>>, grace: Option<Duration>) -> Vec<u8> {
    match grace {
        None => rx.recv().unwrap_or_default(),
        Some(grace) => rx.recv_timeout(grace).unwrap_or_default(),
    }
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
/// Never rendered as the user-safe message (SAD §19, §28); both `args` and
/// `stderr` are redacted before this is constructed (EPIC-22/T-224/US-113):
/// `git`'s own stderr can itself contain a credential-bearing URL (e.g.
/// "fatal: could not read Username for 'https://user:pass@host'"), so
/// redacting only the argument list this process was invoked with would not
/// be enough.
#[derive(Debug)]
struct ProcessDiagnostic {
    args: Vec<String>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Renders `bytes` as text with any secret-shaped fragment redacted
/// (EPIC-22/T-224/US-113), for embedding in a [`ProcessDiagnostic`]. Used
/// for both `stdout` and `stderr`: some Git subcommands report their
/// substantive failure detail on stdout rather than stderr (e.g. `git stash
/// apply`/`pop`'s "CONFLICT" report, EPIC-18/T-218/US-093), so this
/// diagnostic's classification callers (`gitsail-git::provider`'s
/// `classify_*` functions) need both redacted and available, not only
/// stderr.
fn redacted_output(bytes: &[u8]) -> String {
    redact_secrets(&String::from_utf8_lossy(bytes))
}

/// Retained as the pre-EPIC-18 name for the same redaction, since `stderr`
/// is what every existing call site actually passes.
fn redacted_stderr(stderr: &[u8]) -> String {
    redacted_output(stderr)
}

impl fmt::Display for ProcessDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exit_code={:?} args={:?} stdout={} stderr={}",
            self.exit_code, self.args, self.stdout, self.stderr
        )
    }
}

impl std::error::Error for ProcessDiagnostic {}

/// Keeps Windows from flashing a console window for every Git call.
///
/// The Desktop app is built as a GUI binary (`windows_subsystem =
/// "windows"`), so it owns no console. Windows therefore hands each
/// console child it spawns a brand-new console window — and `git` is a
/// console program, so the user saw a CMD window blink on screen for every
/// single invocation. `CREATE_NO_WINDOW` suppresses that window.
///
/// Applied to every spawn rather than only the Desktop's, because this
/// adapter always captures stdout/stderr through pipes and never writes to
/// a console itself: the CLI and TUI lose nothing by it.
#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

fn process_failure_error(
    safe_args: &[String],
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> GitSailError {
    GitSailError::new(
        ErrorCode::ProcessFailure,
        "git process exited with a non-zero status",
    )
    .with_source(ProcessDiagnostic {
        args: safe_args.to_vec(),
        exit_code,
        stdout: redacted_output(stdout),
        stderr: redacted_stderr(stderr),
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
        stdout: String::new(),
        stderr: redacted_stderr(stderr),
    })
}

fn cancelled_error(safe_args: &[String], stderr: &[u8]) -> GitSailError {
    GitSailError::new(ErrorCode::Cancelled, "git process was cancelled").with_source(
        ProcessDiagnostic {
            args: safe_args.to_vec(),
            exit_code: None,
            stdout: String::new(),
            stderr: redacted_stderr(stderr),
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

    /// T-256/US-123 criterion 3 (installation matrix): before this test,
    /// no test anywhere in the workspace exercised `git` genuinely missing
    /// (or a configured path pointing at a nonexistent executable) —
    /// `GitProcessRunnerConfig::default()` always found a real `git` on
    /// every developer/CI machine this crate's other tests ran on, so this
    /// path was correct-by-reading but never proven to actually return
    /// `ErrorCode::GitNotInstalled` (the error every interface's own
    /// "install Git" onboarding message keys off — see
    /// `gitsail-cli::exit_code::exit_code_for`'s dedicated mapping for it).
    #[test]
    fn discover_reports_git_not_installed_for_a_nonexistent_override_path() {
        let path = PathBuf::from("/nonexistent/definitely-not-a-real-git-binary");
        let error = GitExecutable::discover(Some(&path))
            .expect_err("a nonexistent git path must never resolve to an executable");
        assert_eq!(error.code(), ErrorCode::GitNotInstalled);
    }

    /// Same hazard as above, exercised through
    /// `GitProcessRunner::new`/`GitProcessRunnerConfig` — the actual
    /// constructor every adapter (`GitCliProvider`) and every interface
    /// (TUI, Desktop, `gitsail-cli`) calls, not just the lower-level
    /// `GitExecutable::discover` it wraps.
    #[test]
    fn runner_new_reports_git_not_installed_for_a_configured_nonexistent_path() {
        let config = GitProcessRunnerConfig {
            executable: Some(PathBuf::from(
                "/nonexistent/definitely-not-a-real-git-binary",
            )),
            ..GitProcessRunnerConfig::default()
        };
        match GitProcessRunner::new(config) {
            Ok(_) => panic!("a nonexistent configured git path must never construct a runner"),
            Err(error) => assert_eq!(error.code(), ErrorCode::GitNotInstalled),
        }
    }
}
