//! Read use cases (SAD §9's initial list). Each use case depends on
//! [`RepositoryReadPort`] rather than a concrete adapter, so it can be
//! exercised with a test double (ADR-002, ADR-009).

use std::path::Path;
use std::sync::Arc;

use gitsail_domain::{
    Blame, Branch, Commit, CommitHash, Diff, GitSailError, Repository, RepositoryStatus,
};

use crate::ports::{CommitQuery, DiffRequest, Page, RepositoryReadPort};

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

    pub fn execute(&self, repo: &Repository, request: &DiffRequest) -> Result<Diff, GitSailError> {
        self.port.diff(repo, request)
    }
}

pub struct GetFileBlame {
    port: Arc<dyn RepositoryReadPort>,
}

impl GetFileBlame {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self { port }
    }

    pub fn execute(
        &self,
        repo: &Repository,
        file: &Path,
        revision: Option<&CommitHash>,
    ) -> Result<Blame, GitSailError> {
        self.port.blame(repo, file, revision)
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
        received_commit_query: Mutex<Option<CommitQuery>>,
        received_diff_request: Mutex<Option<DiffRequest>>,
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
                blame: Blame { lines: vec![] },
                received_commit_query: Mutex::new(None),
                received_diff_request: Mutex::new(None),
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

        fn diff(&self, _repo: &Repository, request: &DiffRequest) -> Result<Diff, GitSailError> {
            *self.received_diff_request.lock().unwrap() = Some(request.clone());
            Ok(self.diff.clone())
        }

        fn blame(
            &self,
            _repo: &Repository,
            _file: &Path,
            _revision: Option<&CommitHash>,
        ) -> Result<Blame, GitSailError> {
            Ok(self.blame.clone())
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

        let diff = use_case.execute(&port.repository, &request).unwrap();

        assert_eq!(diff, port.diff);
        assert_eq!(*port.received_diff_request.lock().unwrap(), Some(request));
    }

    #[test]
    fn get_file_blame_delegates_to_port() {
        let port = Arc::new(FakeReadPort::new());
        let use_case = GetFileBlame::new(port.clone());

        let blame = use_case
            .execute(&port.repository, Path::new("src/lib.rs"), None)
            .unwrap();

        assert_eq!(blame, port.blame);
    }
}
