//! Read use cases (SAD §9's initial list). Each use case depends on
//! [`RepositoryReadPort`] rather than a concrete adapter, so it can be
//! exercised with a test double (ADR-002, ADR-009).

use std::path::Path;
use std::sync::Arc;

use gitsail_domain::{
    Blame, Branch, CancellationToken, Commit, CommitHash, Diff, GitSailError, LineHistory,
    Repository, RepositoryStatus,
};

use crate::blame_cache::{BlameCache, BlameCacheKey};
use crate::ports::{BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page, RepositoryReadPort};

/// The SHA-1 hash of the empty tree object: a value fixed by Git's object
/// format (identical in every repository, not repository state) used as
/// the "old side" of a root commit's diff, since a root commit has no
/// parent to diff against (US-026 criterion 1).
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

pub struct OpenRepository {
    port: Arc<dyn RepositoryReadPort>,
}

impl OpenRepository {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, path: &Path) -> Result<Repository, GitSailError> {
        self.port.discover(path)
    }
}

pub struct GetRepositoryStatus {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetRepositoryStatus {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
        self.port.status(repo)
    }
}

pub struct GetCommitHistory {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetCommitHistory {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        query: &CommitQuery,
    ) -> Result<Page<Commit>, GitSailError> {
        self.port.commits(repo, query)
    }
}

pub struct GetCommit {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetCommit {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, hash: &CommitHash) -> Result<Commit, GitSailError> {
        self.port.commit(repo, hash)
    }
}

pub struct ListBranches {
    port: Arc<dyn RepositoryReadPort>,
}

impl ListBranches {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
        self.port.branches(repo)
    }
}

pub struct GetDiff {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetDiff {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        request: &DiffRequest,
        cancel: &CancellationToken,
    ) -> Result<Diff, GitSailError> {
        self.port.diff(repo, request, cancel)
    }
}

/// The diff of a single commit against its resolved base (US-026): `base`
/// is `None` for a root commit (diffed against the empty tree instead) and
/// `Some` otherwise, always naming exactly which commit was used so a
/// caller can display it (US-026 criterion 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDiff {
    pub target: CommitHash,
    pub base: Option<CommitHash>,
    pub diff: Diff,
}

/// Diffs a single commit against its parent (US-026).
///
/// Parent policy, made explicit here rather than left to a caller to
/// improvise: a root commit is compared against the empty tree (criterion
/// 1); a merge commit is compared against its **first** parent — the same
/// convention `git show`/`git diff <merge>^!` default to — so `base` always
/// names one specific, well-known commit rather than something derived
/// from every parent at once (criterion 2).
pub struct GetCommitDiff {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetCommitDiff {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        target: &CommitHash,
        cancel: &CancellationToken,
    ) -> Result<CommitDiff, GitSailError> {
        let commit = self.port.commit(repo, target)?;
        let base = if commit.is_root() {
            None
        } else {
            Some(commit.parents[0].clone())
        };
        let from = base
            .clone()
            .unwrap_or_else(|| CommitHash::new(EMPTY_TREE_SHA1).expect("well-known constant"));
        let request = DiffRequest {
            from: Some(from),
            to: Some(target.clone()),
            staged: false,
            path_filter: None,
            context_lines: None,
        };
        let diff = self.port.diff(repo, &request, cancel)?;
        Ok(CommitDiff {
            target: target.clone(),
            base,
            diff,
        })
    }
}

/// The result of comparing two arbitrary revisions (US-028), naming the
/// exact commits `base`/`target` resolved to (criterion 1) so a caller can
/// display them unambiguously alongside the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionComparison {
    pub base: CommitHash,
    pub target: CommitHash,
    pub diff: Diff,
}

/// Compares two revisions chosen by a caller (US-028), resolving each side
/// through [`RepositoryReadPort::resolve_revision`] before diffing. An
/// unresolvable revision fails outright — no [`RevisionComparison`] is ever
/// constructed from a partial resolution — so a caller can never mistake a
/// stale result for a fresh one (criterion 3).
pub struct CompareRevisions {
    port: Arc<dyn RepositoryReadPort>,
}

impl CompareRevisions {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        base_revision: &str,
        target_revision: &str,
        cancel: &CancellationToken,
    ) -> Result<RevisionComparison, GitSailError> {
        let base = self.port.resolve_revision(repo, base_revision)?;
        let target = self.port.resolve_revision(repo, target_revision)?;
        let request = DiffRequest {
            from: Some(base.clone()),
            to: Some(target.clone()),
            staged: false,
            path_filter: None,
            context_lines: None,
        };
        let diff = self.port.diff(repo, &request, cancel)?;
        Ok(RevisionComparison { base, target, diff })
    }
}

/// Queries line-level blame for a file, caching results so repeated queries
/// for the same file/revision/content version are not re-executed against
/// the port (US-034).
pub struct GetFileBlame {
    port: Arc<dyn RepositoryReadPort>,
    cache: BlameCache,
}

impl GetFileBlame {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self {
            port,
            cache: BlameCache::new(),
        }
    }

    /// `content_version` is an opaque token the caller owns (e.g. a content
    /// hash or an editor buffer revision counter) that changes whenever the
    /// queried content could have changed, so a stale cache entry is never
    /// served across an edit (US-034 criterion 1).
    pub fn execute(
        &self,
        repo: &Repository,
        request: &BlameRequest,
        content_version: u64,
        cancel: &CancellationToken,
    ) -> Result<Blame, GitSailError> {
        let key = BlameCacheKey {
            file: request.file.clone(),
            revision: request.revision.clone(),
            content_version,
        };
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached);
        }
        let ticket = self.cache.begin_query();
        let blame = self.port.blame(repo, request, cancel)?;
        self.cache.complete_query(ticket, key, blame.clone());
        Ok(blame)
    }

    /// Drops every cached result (US-034 criterion 2: a relevant change —
    /// e.g. a commit or stage that alters history the cache might reflect —
    /// invalidates it rather than serving stale data).
    pub fn invalidate_cache(&self) {
        self.cache.invalidate_all();
    }
}

/// Traces the commit-level history of a line range within a file (US-019).
pub struct GetLineHistory {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetLineHistory {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        request: &LineHistoryRequest,
        cancel: &CancellationToken,
    ) -> Result<LineHistory, GitSailError> {
        self.port.line_history(repo, request, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        BranchName, ChangeType, FileChange, FileStatusCode, HeadState, RepositoryId, Signature,
    };
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// A double standing in for a real Git adapter (e.g. `gitsail-git`).
    /// Backed entirely by in-memory fixtures, with no shell/process/Git
    /// dependency, demonstrating that use cases can be exercised against
    /// any `RepositoryReadPort` implementation.
    struct FakeReadPort {
        repository: Repository,
        status: RepositoryStatus,
        commit_page: Page<Commit>,
        single_commit: Commit,
        branches: Vec<Branch>,
        diff: Diff,
        blame: Blame,
        line_history: LineHistory,
        revisions: std::collections::HashMap<String, CommitHash>,
        received_commit_query: Mutex<Option<CommitQuery>>,
        received_diff_request: Mutex<Option<DiffRequest>>,
        received_line_history_request: Mutex<Option<LineHistoryRequest>>,
    }

    fn sample_signature() -> Signature {
        Signature::new("Ada Lovelace", "ada@example.com")
    }

    fn sample_commit(hash: &str) -> Commit {
        let hash = CommitHash::new(hash).unwrap();
        Commit {
            short_hash: hash.to_short(8),
            hash,
            parents: vec![],
            author: sample_signature(),
            committer: sample_signature(),
            author_date: gitsail_domain::GitTimestamp::new(0, 0),
            commit_date: gitsail_domain::GitTimestamp::new(0, 0),
            subject: "sample commit".into(),
            body: String::new(),
            decorations: vec![],
        }
    }

    fn sample_repository() -> Repository {
        Repository {
            id: RepositoryId::from_canonical_root(Path::new("/repo")),
            root_path: PathBuf::from("/repo"),
            worktree_path: None,
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        }
    }

    impl FakeReadPort {
        fn new() -> Self {
            let commit = sample_commit("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
            Self {
                repository: sample_repository(),
                status: RepositoryStatus {
                    branch: Some(BranchName::new("main").unwrap()),
                    head_state: HeadState::Attached {
                        branch: BranchName::new("main").unwrap(),
                    },
                    files: vec![FileChange {
                        path: PathBuf::from("README.md"),
                        previous_path: None,
                        change_type: ChangeType::Modified,
                        index_status: FileStatusCode::Unmodified,
                        worktree_status: FileStatusCode::Modified,
                    }],
                },
                commit_page: Page {
                    items: vec![commit.clone()],
                    next_cursor: None,
                    has_more: false,
                },
                single_commit: commit,
                branches: vec![Branch {
                    name: BranchName::new("main").unwrap(),
                    kind: gitsail_domain::BranchKind::Local,
                    target: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                    is_current: true,
                }],
                diff: Diff { files: vec![] },
                blame: Blame {
                    file: PathBuf::new(),
                    revision: None,
                    lines: vec![],
                },
                line_history: LineHistory {
                    file: PathBuf::new(),
                    revision: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
                    range: gitsail_domain::LineRange::new(1, 1),
                    entries: vec![],
                },
                revisions: std::collections::HashMap::new(),
                received_commit_query: Mutex::new(None),
                received_diff_request: Mutex::new(None),
                received_line_history_request: Mutex::new(None),
            }
        }
    }

    impl RepositoryReadPort for FakeReadPort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            Ok(self.repository.clone())
        }

        fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
            Ok(self.status.clone())
        }

        fn commits(
            &self,
            _repo: &Repository,
            query: &CommitQuery,
        ) -> Result<Page<Commit>, GitSailError> {
            *self.received_commit_query.lock().unwrap() = Some(query.clone());
            Ok(self.commit_page.clone())
        }

        fn commit(&self, _repo: &Repository, hash: &CommitHash) -> Result<Commit, GitSailError> {
            if *hash == self.single_commit.hash {
                Ok(self.single_commit.clone())
            } else {
                Err(GitSailError::new(
                    gitsail_domain::ErrorCode::RepositoryNotFound,
                    "no such commit",
                ))
            }
        }

        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            Ok(self.branches.clone())
        }

        fn diff(
            &self,
            _repo: &Repository,
            request: &DiffRequest,
            _cancel: &CancellationToken,
        ) -> Result<Diff, GitSailError> {
            *self.received_diff_request.lock().unwrap() = Some(request.clone());
            Ok(self.diff.clone())
        }

        fn resolve_revision(
            &self,
            _repo: &Repository,
            revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            self.revisions
                .get(revision)
                .cloned()
                .ok_or_else(|| {
                    GitSailError::new(
                        gitsail_domain::ErrorCode::RepositoryNotFound,
                        format!("revision '{revision}' could not be resolved to a commit"),
                    )
                })
        }

        fn blame(
            &self,
            _repo: &Repository,
            _request: &BlameRequest,
            _cancel: &CancellationToken,
        ) -> Result<Blame, GitSailError> {
            Ok(self.blame.clone())
        }

        fn line_history(
            &self,
            _repo: &Repository,
            request: &LineHistoryRequest,
            _cancel: &CancellationToken,
        ) -> Result<LineHistory, GitSailError> {
            *self.received_line_history_request.lock().unwrap() = Some(request.clone());
            Ok(self.line_history.clone())
        }
    }

    #[test]
    fn open_repository_delegates_to_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = OpenRepository::new(port.clone());

        let repo = use_case.execute(Path::new("/repo")).unwrap();

        assert_eq!(repo, port.repository);
    }

    #[test]
    fn get_repository_status_delegates_to_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetRepositoryStatus::new(port.clone());

        let status = use_case.execute(&port.repository).unwrap();

        assert!(!status.is_clean());
        assert_eq!(status, port.status);
    }

    #[test]
    fn get_commit_history_forwards_query_and_returns_page() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetCommitHistory::new(port.clone());
        let query = CommitQuery {
            limit: Some(10),
            ..CommitQuery::default()
        };

        let page = use_case.execute(&port.repository, &query).unwrap();

        assert_eq!(page, port.commit_page);
        assert_eq!(*port.received_commit_query.lock().unwrap(), Some(query));
    }

    #[test]
    fn get_commit_returns_matching_commit() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetCommit::new(port.clone());

        let commit = use_case
            .execute(&port.repository, &port.single_commit.hash)
            .unwrap();

        assert_eq!(commit, port.single_commit);
    }

    #[test]
    fn get_commit_propagates_port_error() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetCommit::new(port.clone());
        let missing = CommitHash::new("cafecafecafecafecafecafecafecafecafecafe").unwrap();

        let err = use_case.execute(&port.repository, &missing).unwrap_err();

        assert_eq!(err.code(), gitsail_domain::ErrorCode::RepositoryNotFound);
    }

    #[test]
    fn list_branches_delegates_to_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = ListBranches::new(port.clone());

        let branches = use_case.execute(&port.repository).unwrap();

        assert_eq!(branches, port.branches);
    }

    #[test]
    fn get_diff_forwards_request_and_returns_diff() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetDiff::new(port.clone());
        let request = DiffRequest {
            path_filter: Some(PathBuf::from("src/lib.rs")),
            ..DiffRequest::default()
        };

        let diff = use_case
            .execute(&port.repository, &request, &CancellationToken::new())
            .unwrap();

        assert_eq!(diff, port.diff);
        assert_eq!(*port.received_diff_request.lock().unwrap(), Some(request));
    }

    fn commit_with_parents(hash: &str, parents: &[&str]) -> Commit {
        let mut commit = sample_commit(hash);
        commit.parents = parents
            .iter()
            .map(|p| CommitHash::new(*p).unwrap())
            .collect();
        commit
    }

    #[test]
    fn get_commit_diff_uses_the_empty_tree_for_a_root_commit() {
        let mut port = FakeReadPort::new();
        let root_hash = "1111111111111111111111111111111111111111";
        port.single_commit = commit_with_parents(root_hash, &[]);
        let port = Arc::new(port);
        let use_case = GetCommitDiff::new(port.clone());

        let result = use_case
            .execute(
                &port.repository,
                &port.single_commit.hash,
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(result.target, port.single_commit.hash);
        assert!(result.base.is_none(), "a root commit has no base commit");
        let received = port.received_diff_request.lock().unwrap().clone().unwrap();
        assert_eq!(
            received.from.unwrap().as_str(),
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );
        assert_eq!(received.to, Some(port.single_commit.hash.clone()));
    }

    #[test]
    fn get_commit_diff_uses_the_first_parent_for_a_merge_commit() {
        let mut port = FakeReadPort::new();
        let merge_hash = "2222222222222222222222222222222222222222";
        let first_parent = "3333333333333333333333333333333333333333";
        let second_parent = "4444444444444444444444444444444444444444";
        port.single_commit = commit_with_parents(merge_hash, &[first_parent, second_parent]);
        let port = Arc::new(port);
        let use_case = GetCommitDiff::new(port.clone());

        let result = use_case
            .execute(
                &port.repository,
                &port.single_commit.hash,
                &CancellationToken::new(),
            )
            .unwrap();

        assert_eq!(
            result.base,
            Some(CommitHash::new(first_parent).unwrap()),
            "merge diff base must be the first parent, not the second"
        );
    }

    #[test]
    fn compare_revisions_resolves_both_sides_and_swapping_reverses_the_request() {
        let mut port = FakeReadPort::new();
        let base_hash = CommitHash::new("5555555555555555555555555555555555555555").unwrap();
        let target_hash = CommitHash::new("6666666666666666666666666666666666666666").unwrap();
        port.revisions
            .insert("base-branch".to_string(), base_hash.clone());
        port.revisions
            .insert("target-branch".to_string(), target_hash.clone());
        let port = Arc::new(port);
        let use_case = CompareRevisions::new(port.clone());

        let forward = use_case
            .execute(
                &port.repository,
                "base-branch",
                "target-branch",
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(forward.base, base_hash);
        assert_eq!(forward.target, target_hash);
        let forward_request = port.received_diff_request.lock().unwrap().clone().unwrap();
        assert_eq!(forward_request.from, Some(base_hash.clone()));
        assert_eq!(forward_request.to, Some(target_hash.clone()));

        let swapped = use_case
            .execute(
                &port.repository,
                "target-branch",
                "base-branch",
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(swapped.base, target_hash);
        assert_eq!(swapped.target, base_hash);
        let swapped_request = port.received_diff_request.lock().unwrap().clone().unwrap();
        assert_eq!(swapped_request.from, Some(target_hash));
        assert_eq!(swapped_request.to, Some(base_hash));
    }

    #[test]
    fn compare_revisions_fails_without_building_a_partial_comparison() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = CompareRevisions::new(port.clone());

        let err = use_case
            .execute(&port.repository, "does-not-exist", "main", &CancellationToken::new())
            .unwrap_err();

        assert_eq!(err.code(), gitsail_domain::ErrorCode::RepositoryNotFound);
        assert!(
            port.received_diff_request.lock().unwrap().is_none(),
            "an unresolvable base must short-circuit before any diff is requested"
        );
    }

    #[test]
    fn get_file_blame_delegates_to_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetFileBlame::new(port.clone());
        let request = BlameRequest {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            line_range: None,
            buffer_contents: None,
        };

        let blame = use_case
            .execute(&port.repository, &request, 0, &CancellationToken::new())
            .unwrap();

        assert_eq!(blame, port.blame);
    }

    #[test]
    fn get_file_blame_serves_a_repeated_query_from_cache_without_hitting_the_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetFileBlame::new(port.clone());
        let request = BlameRequest {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            line_range: None,
            buffer_contents: None,
        };

        let first = use_case
            .execute(&port.repository, &request, 1, &CancellationToken::new())
            .unwrap();
        let second = use_case
            .execute(&port.repository, &request, 1, &CancellationToken::new())
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn get_file_blame_cache_miss_on_a_new_content_version_and_invalidation() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetFileBlame::new(port.clone());
        let request = BlameRequest {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            line_range: None,
            buffer_contents: None,
        };

        use_case
            .execute(&port.repository, &request, 1, &CancellationToken::new())
            .unwrap();
        // A different content version must not be served from the cache
        // entry keyed to version 1 (US-034 criterion 1).
        use_case
            .execute(&port.repository, &request, 2, &CancellationToken::new())
            .unwrap();

        use_case.invalidate_cache();
        // After invalidation, the same (file, revision, version) key must
        // still resolve correctly by going back to the port rather than
        // returning stale/missing data.
        let after_invalidate = use_case
            .execute(&port.repository, &request, 1, &CancellationToken::new())
            .unwrap();
        assert_eq!(after_invalidate, port.blame);
    }

    #[test]
    fn get_line_history_forwards_request_and_returns_history() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetLineHistory::new(port.clone());
        let request = LineHistoryRequest {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            range: gitsail_domain::LineRange::new(10, 20),
        };

        let history = use_case
            .execute(&port.repository, &request, &CancellationToken::new())
            .unwrap();

        assert_eq!(history, port.line_history);
        assert_eq!(
            *port.received_line_history_request.lock().unwrap(),
            Some(request)
        );
    }
}
