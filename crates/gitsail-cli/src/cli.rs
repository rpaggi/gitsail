//! Command-line surface (SAD §15; US-036, US-037, US-038).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// GitSail — Navigate your Git history.
///
/// Runs one read-only query against a Git repository and prints the result
/// either as human-readable text or, with `--json`, as a single versioned
/// JSON envelope on stdout (SAD §14, §15).
#[derive(Debug, Parser)]
#[command(name = "gitsail", version, about, long_about = None)]
pub struct Cli {
    /// Path to the repository to query (default: current directory).
    #[arg(long, global = true, default_value = ".")]
    pub repo: PathBuf,

    /// Print a single versioned JSON envelope on stdout instead of
    /// human-readable text (US-037). Never mixed with progress, ANSI color
    /// or log output on stdout.
    #[arg(long, global = true)]
    pub json: bool,

    /// Print additional diagnostic detail to stderr. Diagnostics are always
    /// redacted of anything that looks like embedded credentials
    /// (US-038 criterion 3) and never interleave with stdout.
    #[arg(long, global = true)]
    pub debug: bool,

    /// Abort the underlying `git` invocation after this many seconds
    /// (fractional values are accepted, e.g. `0.5`). Unset means no
    /// per-command timeout.
    #[arg(long, global = true)]
    pub timeout: Option<f64>,

    /// Explicit path to the `git` executable (default: `git` on PATH).
    #[arg(long, global = true)]
    pub git_path: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Discover the repository at --repo and print its identity and HEAD state.
    ///
    /// Example: `gitsail open --repo ~/code/gitsail`
    Open,

    /// Show working tree and index status.
    ///
    /// Example: `gitsail status --json`
    Status,

    /// Show commit history.
    ///
    /// Examples:
    ///   gitsail log
    ///   gitsail log main --limit 20 --author ada
    ///   gitsail log --path src/lib.rs --grep "fix"
    Log(LogArgs),

    /// List local and remote-tracking branches.
    ///
    /// Example: `gitsail branches --json`
    Branches,

    /// Show a diff.
    ///
    /// Examples:
    ///   gitsail diff                  # unstaged changes (working tree vs index)
    ///   gitsail diff --staged         # staged changes (index vs HEAD)
    ///   gitsail diff main             # main vs the working tree
    ///   gitsail diff main feature     # main vs feature
    Diff(DiffArgs),

    /// Show line-by-line attribution for a file.
    ///
    /// Examples:
    ///   gitsail blame src/lib.rs
    ///   gitsail blame src/lib.rs --revision HEAD~5 --range 10-25
    Blame(BlameArgs),

    /// Show a single commit's full details.
    ///
    /// Example: `gitsail commit HEAD~2`
    Commit(CommitArgs),

    /// Show a single commit's diff against its resolved base (root commit ->
    /// empty tree, merge commit -> first parent — same convention as `git show`).
    ///
    /// Example: `gitsail commit-diff abc123`
    CommitDiff(CommitDiffArgs),

    /// Show the commit-level history of a line range within a file.
    ///
    /// Example: `gitsail line-history src/lib.rs --range 10-25 --revision HEAD`
    LineHistory(LineHistoryArgs),

    /// Show a file's content as of a specific revision (e.g. to open a
    /// historical version read-only).
    ///
    /// Example: `gitsail show-file src/lib.rs --revision HEAD~3`
    ShowFile(ShowFileArgs),
}

#[derive(Debug, Args)]
pub struct LogArgs {
    /// Revision or revision range in Git's own syntax (default: HEAD).
    pub revision: Option<String>,

    /// Maximum number of commits to return (default: 50).
    #[arg(long)]
    pub limit: Option<u32>,

    /// Opaque continuation token from a previous page's `nextCursor`.
    #[arg(long)]
    pub cursor: Option<String>,

    /// Only commits whose author name/email contains this text.
    #[arg(long)]
    pub author: Option<String>,

    /// Only commits whose message contains this text.
    #[arg(long = "grep")]
    pub text_query: Option<String>,

    /// Only commits that touch this path.
    #[arg(long = "path")]
    pub path_filter: Option<PathBuf>,

    /// With --path, stop at rename boundaries instead of following the
    /// file's history under its former name(s).
    #[arg(long)]
    pub no_follow: bool,
}

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Base revision. Omit to diff the working tree/index (or, with
    /// --staged, the index against HEAD).
    pub base: Option<String>,

    /// Target revision. Requires `base`; omit to diff `base` against the
    /// working tree.
    pub target: Option<String>,

    /// Compare the index against HEAD (`git diff --cached`). Cannot be
    /// combined with a revision.
    #[arg(long)]
    pub staged: bool,

    /// Restrict the diff to this path.
    #[arg(long = "path")]
    pub path_filter: Option<PathBuf>,

    /// Number of context lines around each change (default: 3).
    #[arg(long)]
    pub context: Option<u32>,
}

#[derive(Debug, Args)]
pub struct BlameArgs {
    /// File to blame, relative to the repository.
    pub file: PathBuf,

    /// Revision to blame at (default: the working tree, including
    /// uncommitted changes).
    #[arg(long)]
    pub revision: Option<String>,

    /// Inclusive 1-based line range, e.g. `10-25`.
    #[arg(long)]
    pub range: Option<String>,
}

#[derive(Debug, Args)]
pub struct CommitArgs {
    /// Commit-ish to show (hash, branch, tag, or other revision expression).
    pub revision: String,
}

#[derive(Debug, Args)]
pub struct CommitDiffArgs {
    pub revision: String,
}

#[derive(Debug, Args)]
pub struct LineHistoryArgs {
    /// File to trace, relative to the repository.
    pub file: PathBuf,
    /// Revision to start from (default: HEAD).
    #[arg(long)]
    pub revision: Option<String>,
    /// Inclusive 1-based line range, e.g. `10-25`.
    #[arg(long)]
    pub range: String,
}

#[derive(Debug, Args)]
pub struct ShowFileArgs {
    /// File to read, relative to the repository.
    pub file: PathBuf,
    #[arg(long)]
    pub revision: String,
}
