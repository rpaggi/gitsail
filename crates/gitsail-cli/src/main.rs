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
use gitsail_domain::{CancellationToken, GitSailError};
use gitsail_git::{redact_credentials, GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
use gitsail_protocol::{Envelope, ErrorPayload, RequestId, SCHEMA_VERSION};

use cli::Cli;
use exit_code::exit_code_for;
use output::Output;

fn main() {
    let cli = Cli::parse();
    let cancel = CancellationToken::new();
    install_signal_handler(cancel.clone());

    let request_id = RequestId::generate();
    let exit_code = run(&cli, &cancel, &request_id);
    std::process::exit(exit_code);
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

    let result = commands::execute(&port, &cli.repo, &cli.command, cancel);
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
            redact_credentials(&diagnostic.to_string())
        );
    }
}
