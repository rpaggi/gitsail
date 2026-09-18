//! Tauri commands (SAD §17: "Tauri commands should be thin adapters.
//! Business logic belongs in application/domain crates.").
//!
//! This is the only module in this crate with `#[tauri::command]`, and each
//! `#[tauri::command]` function is a single line delegating to a plain,
//! directly testable `*_impl` function below it. Every `*_impl` function
//! does exactly three things: call one `gitsail-application` use case, map
//! the result through a `gitsail-protocol` DTO, and map any error through
//! `ErrorPayload`. No Git argument construction, no output parsing, and no
//! business rule beyond that lives here (US-051 criterion 3) — this file is
//! what a "bridge review" against SAD §17 inspects.
//!
//! Tauri v2 dispatches commands off the webview/UI thread by default, so
//! the synchronous `RepositoryReadPort` calls below never block rendering
//! (SAD §26's "UI thread/render loop never waits on Git process
//! execution").

use std::path::Path;

use gitsail_application::{OpenRepository, RefreshReason};
use gitsail_protocol::{ErrorPayload, RepositoryDto, RepositoryStatusDto};

use crate::state::AppState;

#[tauri::command]
pub fn open_repository(
    path: String,
    state: tauri::State<AppState>,
) -> Result<RepositoryDto, ErrorPayload> {
    open_repository_impl(&state, &path)
}

#[tauri::command]
pub fn get_repository_status(
    state: tauri::State<AppState>,
) -> Result<RepositoryStatusDto, ErrorPayload> {
    get_repository_status_impl(&state)
}

fn open_repository_impl(state: &AppState, path: &str) -> Result<RepositoryDto, ErrorPayload> {
    let repository = OpenRepository::new(state.port())
        .execute(Path::new(path))
        .map_err(|err| ErrorPayload::from(&err))?;
    let dto = RepositoryDto::from(&repository);
    state.open_session(repository);
    Ok(dto)
}

fn get_repository_status_impl(state: &AppState) -> Result<RepositoryStatusDto, ErrorPayload> {
    state
        .with_session_mut(|session| {
            session.refresh(RefreshReason::Manual)?;
            Ok(RepositoryStatusDto::from(session.status().expect(
                "status is always Some immediately after a successful refresh",
            )))
        })
        .map_err(|err| ErrorPayload::from(&err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    use gitsail_application::{BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page};
    use gitsail_domain::{
        Blame, Branch, BranchName, CancellationToken, ChangeType, Commit, CommitHash, FileChange,
        FileStatusCode, GitSailError, HeadState, LineHistory, Repository, RepositoryId,
        RepositoryStatus,
    };

    /// A minimal `RepositoryReadPort` double exercising only `discover` and
    /// `status`, the two operations this story's commands use — every other
    /// method is unreachable from these tests and left `unimplemented!()`,
    /// matching the pattern already used by
    /// `gitsail-application/src/session.rs`'s own test doubles.
    struct FakePort {
        repository: Repository,
        status: RepositoryStatus,
    }

    impl gitsail_application::RepositoryReadPort for FakePort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            Ok(self.repository.clone())
        }

        fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
            Ok(self.status.clone())
        }

        fn commits(
            &self,
            _repo: &Repository,
            _query: &CommitQuery,
        ) -> Result<Page<Commit>, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn commit(&self, _repo: &Repository, _hash: &CommitHash) -> Result<Commit, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn diff(
            &self,
            _repo: &Repository,
            _request: &DiffRequest,
            _cancel: &CancellationToken,
        ) -> Result<gitsail_domain::Diff, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn resolve_revision(
            &self,
            _repo: &Repository,
            _revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn blame(
            &self,
            _repo: &Repository,
            _request: &BlameRequest,
            _cancel: &CancellationToken,
        ) -> Result<Blame, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn line_history(
            &self,
            _repo: &Repository,
            _request: &LineHistoryRequest,
            _cancel: &CancellationToken,
        ) -> Result<LineHistory, GitSailError> {
            unimplemented!("not exercised by these tests")
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

    fn dirty_status() -> RepositoryStatus {
        RepositoryStatus {
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
        }
    }

    fn state_with_fake_port() -> AppState {
        let port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(FakePort {
            repository: sample_repository(),
            status: dirty_status(),
        });
        AppState::new(port)
    }

    #[test]
    fn open_repository_returns_the_dto_shape_of_the_discovered_repository() {
        let state = state_with_fake_port();

        let dto = open_repository_impl(&state, "/repo").unwrap();

        assert_eq!(dto.root_path, "/repo");
        assert_eq!(dto.current_branch.as_deref(), Some("main"));
    }

    #[test]
    fn get_repository_status_before_opening_fails_with_invalid_repository_state() {
        let state = state_with_fake_port();

        let err = get_repository_status_impl(&state).unwrap_err();

        assert_eq!(err.code, "invalid_repository_state");
    }

    #[test]
    fn get_repository_status_after_opening_returns_the_refreshed_status() {
        let state = state_with_fake_port();
        open_repository_impl(&state, "/repo").unwrap();

        let status = get_repository_status_impl(&state).unwrap();

        assert_eq!(status.files.len(), 1);
    }
}
