//! Read-side port for repository inspection (SAD §9, §10; ADR-002, ADR-009).
//!
//! Mutation ports (StageFiles, CreateCommit, CheckoutBranch, ...) are
//! deliberately out of scope here and land starting v0.2 (ADR-009): keeping
//! them separate means a read-only consumer never gains write capability.

use std::path::{Path, PathBuf};

use gitsail_domain::{
    Blame, Branch, BranchName, CancellationToken, Commit, CommitHash, Diff, GitSailError,
    LineHistory, LineRange, Repository, RepositoryStatus,
};

/// A single page of results plus continuation metadata (SAD §25).
///
/// History must never assume the full repository log fits in memory, so
/// history reads are paginated rather than returning a `Vec` outright.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Filters and pagination for [`RepositoryReadPort::commits`] (SAD §25).
///
/// `path_filter`, when set, scopes history to commits that touch that path
/// (US-018) rather than the whole repository. `follow_renames` makes that
/// scoping policy explicit rather than an implicit adapter default: `true`
/// (the default here) also surfaces the file's history under its former
/// name(s) across renames, matching what "follow this file's evolution"
/// means to a caller; `false` stops at the rename boundary, showing only
/// commits under the current name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    /// A revision or revision range in Git's own syntax (e.g. `main`,
    /// `abc123..def456`). Ignored when `branch` is set. Defaults to `HEAD`.
    pub revision_range: Option<String>,
    pub branch: Option<BranchName>,
    /// Matched against author name/email as a substring (Git's `--author`),
    /// not an exact match; matching is case-sensitive with the `C` locale
    /// this adapter always pins.
    pub author: Option<String>,
    /// Matched against the commit message as a substring (Git's `--grep`).
    /// Only the message is searched, never diff content.
    pub text_query: Option<String>,
    pub path_filter: Option<PathBuf>,
    pub follow_renames: bool,
}

impl Default for CommitQuery {
    fn default() -> Self {
        Self {
            limit: None,
            cursor: None,
            revision_range: None,
            branch: None,
            author: None,
            text_query: None,
            path_filter: None,
            follow_renames: true,
        }
    }
}

/// Selects the two sides and scope of a diff request.
///
/// `from`/`to` of `None` mean "working tree"/"index" respectively, matching
/// how `git diff` treats missing revisions; the exact resolution is an
/// adapter concern, not something this port dictates.
///
/// `staged: true` instead compares the index against a tree (`from`,
/// defaulting to `HEAD`; `to` is not meaningful in this mode and must be
/// `None`), i.e. `git diff --cached` — the diff hunk-level staging (US-013)
/// unstages against, since that is not expressible with `from`/`to` alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffRequest {
    pub from: Option<CommitHash>,
    pub to: Option<CommitHash>,
    pub staged: bool,
    pub path_filter: Option<PathBuf>,
    pub context_lines: Option<u32>,
}

/// Selects the file, revision, optional line range, and optional explicit
/// buffer content for a blame query (US-031, US-032, US-033).
///
/// `revision: None` blames the working tree, including any uncommitted
/// changes (US-033) — mirroring how [`DiffRequest`]'s `from`/`to` treat a
/// missing revision as "working tree". `line_range: None` blames the whole
/// file. `buffer_contents`, when set, blames that exact content instead of
/// reading the file from disk (US-033 criterion 3: an unsaved editor buffer
/// must never be silently treated as identical to the on-disk file); it is
/// only meaningful when `revision` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameRequest {
    pub file: PathBuf,
    pub revision: Option<CommitHash>,
    pub line_range: Option<LineRange>,
    pub buffer_contents: Option<Vec<u8>>,
}

/// Selects the file, range, and starting revision for a line-history query
/// (US-019). `revision: None` defaults to `HEAD`; unlike [`BlameRequest`],
/// there is no working-tree/buffer option — line history only exists for
/// committed revisions (US-019 criterion 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHistoryRequest {
    pub file: PathBuf,
    pub revision: Option<CommitHash>,
    pub range: LineRange,
}

/// Read-only inspection of a Git repository (SAD §9's initial read use
/// cases, SAD §10). Adapters (e.g. `gitsail-git`) implement this trait
/// against a real Git provider; application use cases and tests depend on
/// the trait, never on a concrete adapter.
pub trait RepositoryReadPort {
    fn discover(&self, path: &Path) -> Result<Repository, GitSailError>;
    fn status(&self, repo: &Repository) -> Result<RepositoryStatus, GitSailError>;
    fn commits(&self, repo: &Repository, query: &CommitQuery)
        -> Result<Page<Commit>, GitSailError>;
    fn commit(&self, repo: &Repository, hash: &CommitHash) -> Result<Commit, GitSailError>;
    fn branches(&self, repo: &Repository) -> Result<Vec<Branch>, GitSailError>;
    /// `cancel` lets a caller stop a long-running diff from another thread
    /// (SAD §32; US-027 criterion 3) — check `cancel.is_cancelled()`
    /// pre-call, or pass a fresh [`CancellationToken`] when cancellation is
    /// not needed.
    fn diff(
        &self,
        repo: &Repository,
        request: &DiffRequest,
        cancel: &CancellationToken,
    ) -> Result<Diff, GitSailError>;
    /// Resolves `revision` (a branch, tag, or other Git revision
    /// expression, e.g. `main`, `v1.0`, `HEAD~2`) to the single commit it
    /// unambiguously names (US-028 criterion 1). Fails rather than
    /// returning a stale or partial result for a revision that does not
    /// resolve (US-028 criterion 3).
    fn resolve_revision(&self, repo: &Repository, revision: &str) -> Result<CommitHash, GitSailError>;
    /// `cancel` lets a caller stop a long-running blame query from another
    /// thread (US-034 criterion 3), matching [`Self::diff`].
    fn blame(
        &self,
        repo: &Repository,
        request: &BlameRequest,
        cancel: &CancellationToken,
    ) -> Result<Blame, GitSailError>;
    /// Traces the commit-level evolution of a line range (US-019). `cancel`
    /// matches [`Self::diff`]/[`Self::blame`]: a caller can stop a
    /// long-running query from another thread.
    fn line_history(
        &self,
        repo: &Repository,
        request: &LineHistoryRequest,
        cancel: &CancellationToken,
    ) -> Result<LineHistory, GitSailError>;
}
