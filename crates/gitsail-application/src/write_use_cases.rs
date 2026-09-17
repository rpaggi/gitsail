//! Mutation use cases (SAD §9's v0.2/v0.3 list). Each depends on
//! [`RepositoryWritePort`] rather than a concrete adapter, so it can be
//! exercised with a test double (ADR-002, ADR-009), mirroring
//! `use_cases.rs`'s read-side pattern.

use std::path::PathBuf;
use std::sync::Arc;

use gitsail_domain::{CommitHash, FileDiff, GitSailError, Repository};

use crate::write_ports::RepositoryWritePort;

pub struct StageFiles {
    port: Arc<dyn RepositoryWritePort>,
}

impl StageFiles {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        self.port.stage_files(repo, paths)
    }
}

pub struct UnstageFiles {
    port: Arc<dyn RepositoryWritePort>,
}

impl UnstageFiles {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        self.port.unstage_files(repo, paths)
    }
}

pub struct CreateCommit {
    port: Arc<dyn RepositoryWritePort>,
}

impl CreateCommit {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError> {
        self.port.create_commit(repo, message)
    }
}

pub struct StageHunks {
    port: Arc<dyn RepositoryWritePort>,
}

impl StageHunks {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        self.port.stage_hunks(repo, selection)
    }
}

pub struct UnstageHunks {
    port: Arc<dyn RepositoryWritePort>,
}

impl UnstageHunks {
    pub fn new(port: Arc<dyn RepositoryWritePort>) -> Self {
        Self { port }
    }

    pub fn execute(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        self.port.unstage_hunks(repo, selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{BranchName, ErrorCode, HeadState, RepositoryId};
    use std::path::Path;
    use std::sync::Mutex;

    /// A double standing in for a real Git adapter (e.g. `gitsail-git`),
    /// exercised with no shell/process/Git dependency, demonstrating that
    /// these use cases can be exercised against any `RepositoryWritePort`
    /// implementation (mirroring `use_cases.rs`'s `FakeReadPort`).
    struct FakeWritePort {
        commit_hash: CommitHash,
        fail: bool,
        received_stage: Mutex<Option<Vec<PathBuf>>>,
        received_unstage: Mutex<Option<Vec<PathBuf>>>,
        received_commit_message: Mutex<Option<String>>,
        received_stage_hunks: Mutex<Option<Vec<FileDiff>>>,
        received_unstage_hunks: Mutex<Option<Vec<FileDiff>>>,
    }

    impl FakeWritePort {
        fn new() -> Self {
            Self {
                commit_hash: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
                fail: false,
                received_stage: Mutex::new(None),
                received_unstage: Mutex::new(None),
                received_commit_message: Mutex::new(None),
                received_stage_hunks: Mutex::new(None),
                received_unstage_hunks: Mutex::new(None),
            }
        }

        fn failing() -> Self {
            Self {
                fail: true,
                ..Self::new()
            }
        }
    }

    impl RepositoryWritePort for FakeWritePort {
        fn stage_files(&self, _repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
            *self.received_stage.lock().unwrap() = Some(paths.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale status"));
            }
            Ok(())
        }

        fn unstage_files(&self, _repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
            *self.received_unstage.lock().unwrap() = Some(paths.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale status"));
            }
            Ok(())
        }

        fn create_commit(
            &self,
            _repo: &Repository,
            message: &str,
        ) -> Result<CommitHash, GitSailError> {
            *self.received_commit_message.lock().unwrap() = Some(message.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "nothing staged to commit",
                ));
            }
            Ok(self.commit_hash.clone())
        }

        fn stage_hunks(
            &self,
            _repo: &Repository,
            selection: &[FileDiff],
        ) -> Result<(), GitSailError> {
            *self.received_stage_hunks.lock().unwrap() = Some(selection.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale diff"));
            }
            Ok(())
        }

        fn unstage_hunks(
            &self,
            _repo: &Repository,
            selection: &[FileDiff],
        ) -> Result<(), GitSailError> {
            *self.received_unstage_hunks.lock().unwrap() = Some(selection.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale diff"));
            }
            Ok(())
        }
    }

    fn sample_repository() -> Repository {
        Repository {
            id: RepositoryId::from_canonical_root(Path::new("/repo")),
            root_path: PathBuf::from("/repo"),
            worktree_path: Some(PathBuf::from("/repo")),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        }
    }

    fn sample_file_diff() -> FileDiff {
        FileDiff {
            path: PathBuf::from("a.txt"),
            previous_path: None,
            change_type: gitsail_domain::ChangeType::Modified,
            is_binary: false,
            hunks: vec![],
        }
    }

    #[test]
    fn stage_files_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = StageFiles::new(port.clone());
        let paths = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];

        use_case.execute(&sample_repository(), &paths).unwrap();

        assert_eq!(*port.received_stage.lock().unwrap(), Some(paths));
    }

    #[test]
    fn stage_files_propagates_port_error() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = StageFiles::new(port);

        let err = use_case
            .execute(&sample_repository(), &[PathBuf::from("a.txt")])
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn unstage_files_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = UnstageFiles::new(port.clone());
        let paths = vec![PathBuf::from("a.txt")];

        use_case.execute(&sample_repository(), &paths).unwrap();

        assert_eq!(*port.received_unstage.lock().unwrap(), Some(paths));
    }

    #[test]
    fn create_commit_returns_hash_on_success() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = CreateCommit::new(port.clone());

        let hash = use_case.execute(&sample_repository(), "a message").unwrap();

        assert_eq!(hash, port.commit_hash);
        assert_eq!(
            *port.received_commit_message.lock().unwrap(),
            Some("a message".to_string())
        );
    }

    #[test]
    fn create_commit_propagates_port_error_without_a_false_success() {
        let port = Arc::new(FakeWritePort::failing());
        let use_case = CreateCommit::new(port);

        let err = use_case
            .execute(&sample_repository(), "a message")
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn stage_hunks_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = StageHunks::new(port.clone());
        let selection = vec![sample_file_diff()];

        use_case.execute(&sample_repository(), &selection).unwrap();

        assert_eq!(*port.received_stage_hunks.lock().unwrap(), Some(selection));
    }

    #[test]
    fn unstage_hunks_delegates_to_port() {
        let port = Arc::new(FakeWritePort::new());
        let use_case = UnstageHunks::new(port.clone());
        let selection = vec![sample_file_diff()];

        use_case
            .execute(&sample_repository(), &selection)
            .unwrap();

        assert_eq!(
            *port.received_unstage_hunks.lock().unwrap(),
            Some(selection)
        );
    }
}
