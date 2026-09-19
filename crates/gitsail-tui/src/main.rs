//! `gitsail-tui`: the interactive, keyboard-first GitSail interface (SAD
//! §18, §37; ADR-005; US-040..US-043).
//!
//! This binary is now a thin argument-parsing wrapper — the actual
//! terminal/event loop lives in [`gitsail_tui::run_interactive`] so
//! `gitsail-cli`'s binary can drop into the exact same interface (rather
//! than a second, drifting copy of it) when invoked with no subcommand.
//! See `gitsail_tui::runtime`'s module doc for why the split is there and
//! not here.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// GitSail — interactive terminal interface.
#[derive(Debug, Parser)]
#[command(name = "gitsail-tui", version, about, long_about = None)]
struct Cli {
    /// Path to the repository to open (default: current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Explicit path to the `git` executable (default: `git` on PATH).
    #[arg(long)]
    git_path: Option<PathBuf>,

    /// Disable color and rely on text/markers alone to distinguish state
    /// (US-043 criterion 3). Also enabled automatically when `NO_COLOR` is
    /// set, per that convention.
    #[arg(long)]
    ascii: bool,

    /// Explicit path to a keybindings-override file (T-251/US-109
    /// criterion 2). Defaults to
    /// [`gitsail_tui::keybindings::default_config_path`] (`<OS config
    /// dir>/gitsail/tui/keybindings.conf`) when omitted; a missing file at
    /// either location is not an error — the documented defaults apply.
    #[arg(long)]
    keybindings: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let low_color = cli.ascii || std::env::var_os("NO_COLOR").is_some();
    gitsail_tui::run_interactive(cli.repo, cli.git_path, low_color, cli.keybindings)
}
