//! Read-side port for repository inspection (SAD §9, §10; ADR-002, ADR-009).
//!
//! Mutation ports (StageFiles, CreateCommit, CheckoutBranch, ...) are
//! deliberately out of scope here and land starting v0.2 (ADR-009): keeping
//! them separate means a read-only consumer never gains write capability.

use std::path::{Path, PathBuf};

use gitsail_domain::{
    Blame, Branch, BranchName, Commit, CommitHash, Diff, GitSailError, Repository, RepositoryStatus,
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub revision_range: Option<String>,
    pub branch: Option<BranchName>,
    pub author: Option<String>,
    pub text_query: Option<String>,
}

/// Selects the two sides and scope of a diff request.
///
/// `from`/`to` of `None` mean "working tree"/"index" respectively, matching
/// how `git diff` treats missing revisions; the exact resolution is an
/// adapter concern, not something this port dictates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffRequest {
    pub from: Option<CommitHash>,
    pub to: Option<CommitHash>,
    pub path_filter: Option<PathBuf>,
    pub context_lines: Option<u32>,
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
    fn diff(&self, repo: &Repository, request: &DiffRequest) -> Result<Diff, GitSailError>;
    fn blame(
        &self,
        repo: &Repository,
        file: &Path,
        revision: Option<&CommitHash>,
    ) -> Result<Blame, GitSailError>;
}
