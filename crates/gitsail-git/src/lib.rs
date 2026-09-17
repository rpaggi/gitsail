//! Git CLI infrastructure adapter for GitSail.

#![forbid(unsafe_code)]

pub mod provider;
pub mod runner;

pub use provider::GitCliProvider;
pub use runner::{
    redact_credentials, run_process, CancellationToken, GitExecutable, GitProcessRunner,
    GitProcessRunnerConfig, ProcessOutput, ProcessRequest,
};
