//! `gitsail`: the CLI/JSON protocol front end for GitSail (SAD §15; US-036,
//! US-037, US-038).

#![forbid(unsafe_code)]

mod cli;
mod commands;
mod exit_code;
mod output;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use gitsail_application::RepositoryReadPort;
use gitsail_domain::redact::redact_secrets;
use gitsail_domain::{CancellationToken, GitSailError};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
use gitsail_protocol::{Envelope, ErrorPayload, RequestId, SCHEMA_VERSION};

use cli::{Cli, Command};
use exit_code::exit_code_for;
use output::Output;

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    // No subcommand at all, or the explicit `tui` subcommand, both open
    // the interactive TUI (`cli.rs`'s own doc comment) — intercepted here,
    // before any of the read-only-query machinery (logging, the panic
    // hook, the Ctrl+C-to-cooperative-cancel handler, JSON envelopes) that
    // exists for that machinery alone, none of which the TUI needs or
    // wants (it manages the terminal and Ctrl+C itself).
    if matches!(cli.command, None | Some(Command::Tui)) {
        let low_color = cli.ascii || std::env::var_os("NO_COLOR").is_some();
        return gitsail_tui::run_interactive(cli.repo, cli.git_path, low_color, cli.keybindings);
    }

    init_logging(cli.debug);
    install_panic_hook();
    let cancel = CancellationToken::new();
    install_signal_handler(cancel.clone());

    let request_id = RequestId::generate();
    let exit_code = run(&cli, &cancel, &request_id);
    std::process::exit(exit_code)
}

/// Installs a minimal structured logger (SAD §28; EPIC-22/T-224/US-113):
/// `--debug` raises the level to capture `gitsail-git`'s per-process
/// events (component, operation, duration, error code — see
/// `GitProcessRunner`'s `run_process`), otherwise only warnings/errors are
/// shown. Every event this workspace emits already carries only redacted,
/// structured fields (never raw stdout/stderr or an unredacted argument
/// list), so raising verbosity here can never leak a secret — there is
/// nothing "unlocked" by `--debug` that bypasses redaction.
///
/// Writes to stderr only, exactly like every other diagnostic this binary
/// produces (US-038 criterion 3) — never to a file, and never transmitted
/// anywhere (SAD §28; EPIC-22/T-225/US-114: GitSail sends nothing
/// unsolicited over the network).
fn init_logging(debug: bool) {
    use tracing_subscriber::filter::LevelFilter;
    let level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::WARN
    };
    // Best-effort: a second call (e.g. from a future embedder) or an
    // already-installed global subscriber is not fatal, just a no-op.
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// Chains a panic hook in front of Rust's default one (EPIC-22/T-225/
/// US-114): a panic is logged locally through the same structured,
/// redacted logging path as everything else (in case a panic message
/// happens to embed repository content or a secret-shaped fragment), then
/// the default hook still runs so the usual stderr message/backtrace
/// behavior is unchanged. This never writes anywhere but this process's own
/// stderr and never transmits a crash report anywhere — GitSail has no
/// crash-reporting upload at all, and if one is ever added, sending it
/// requires explicit, informed user consent, never a default-on behavior.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let redacted = redact_secrets(&info.to_string());
        tracing::error!(component = "gitsail-cli", "panic: {redacted}");
        previous(info);
    }));
}

/// Installs a Ctrl+C handler that marks `cancel` instead of letting the
/// default disposition tear this process down mid-write: a diff or blame in
/// flight (the two operations that accept a [`CancellationToken`]) gets the
/// chance to stop cooperatively and this process reports a clean
/// `Cancelled` result with exit code 130 (US-038 criterion 1). Failure to
/// install a handler is not fatal — the command still runs, just without
/// cooperative Ctrl+C handling.
fn install_signal_handler(cancel: CancellationToken) {
    let _ = ctrlc::set_handler(move || cancel.cancel());
}

fn run(cli: &Cli, cancel: &CancellationToken, request_id: &RequestId) -> i32 {
    let runner_config = GitProcessRunnerConfig {
        executable: cli.git_path.clone(),
        default_cwd: None,
        default_timeout: cli.timeout.map(Duration::from_secs_f64),
    };
    let runner = match GitProcessRunner::new(runner_config) {
        Ok(runner) => runner,
        Err(err) => return report(cli, request_id, Err(err)),
    };
    let port: Arc<dyn RepositoryReadPort> = Arc::new(GitCliProvider::new(runner));

    // `main` only calls `run` once `cli.command` is confirmed `Some` (and
    // not `Tui`) — see its own doc comment.
    let command = cli
        .command
        .as_ref()
        .expect("run() is only called once main() has confirmed a non-TUI subcommand");
    let result = commands::execute(&port, &cli.repo, command, cancel);
    report(cli, request_id, result)
}

/// Renders `result` on the channel/format `cli` selected and returns the
/// process exit code for it (US-037, US-038). Stdout and stderr are never
/// mixed: `--json` puts exactly one envelope line on stdout, human mode
/// puts its text on stdout and any error on stderr, and `--debug`
/// diagnostics always go to stderr regardless of format.
fn report(cli: &Cli, request_id: &RequestId, result: Result<Output, GitSailError>) -> i32 {
    match result {
        Ok(output) => {
            if cli.json {
                print_envelope(&Envelope::ok(request_id.clone(), output));
            } else {
                print!("{}", output.render_human());
            }
            exit_code::EXIT_OK
        }
        Err(err) => {
            if cli.debug {
                eprint_debug(&err);
            }
            let code = exit_code_for(err.code());
            if cli.json {
                let payload = ErrorPayload::from(&err);
                print_envelope(&Envelope::<Output>::error(request_id.clone(), payload));
            } else {
                eprintln!("error: {err}");
                if let Some(remediation) = err.remediation() {
                    eprintln!("  {remediation}");
                }
            }
            code
        }
    }
}

fn print_envelope<T: serde::Serialize>(envelope: &Envelope<T>) {
    match serde_json::to_string(envelope) {
        Ok(json) => println!("{json}"),
        Err(_) => println!(
            "{{\"schemaVersion\":{SCHEMA_VERSION},\"status\":\"error\",\"error\":{{\"code\":\"internal\",\"message\":\"failed to serialize response\"}}}}"
        ),
    }
}

/// Prints diagnostic detail for `err` to stderr (US-038 criterion 3). The
/// code/message/operation id are already user-safe by construction, but the
/// diagnostic cause may carry raw Git stderr; it is redacted of anything
/// that looks like embedded credentials before ever leaving the process,
/// the same redaction the Git CLI adapter itself applies to logged
/// arguments.
fn eprint_debug(err: &GitSailError) {
    eprintln!("[debug] code={} message={}", err.code(), err.message());
    if let Some(operation_id) = err.operation_id() {
        eprintln!("[debug] operationId={operation_id}");
    }
    if let Some(diagnostic) = err.diagnostic() {
        eprintln!(
            "[debug] diagnostic={}",
            redact_secrets(&diagnostic.to_string())
        );
    }
}
