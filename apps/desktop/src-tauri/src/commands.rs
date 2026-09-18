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

use gitsail_application::{CommitQuery, GetCommitHistory, OpenRepository, RefreshReason};
use gitsail_domain::{BranchName, GraphCommit};
use gitsail_protocol::{
    CommitGraphPageDto, CommitGraphRowDto, ErrorPayload, RepositoryDto, RepositoryStatusDto,
};

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

/// Loads one page of commit-graph rows (US-067). `reset: true` starts a
/// brand new [`gitsail_domain::CommitGraph`] before laying out this page —
/// used when a filter (e.g. `branch`) changes, so the previous filter's
/// lanes are never mixed into the new one's (US-067 criterion 3, mirroring
/// US-065's "no invented connections" guarantee at the pagination
/// boundary). `reset: false` continues appending to whatever has already
/// been accumulated — a "load more" call.
#[tauri::command]
pub fn get_commit_graph_page(
    branch: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    reset: bool,
    state: tauri::State<AppState>,
) -> Result<CommitGraphPageDto, ErrorPayload> {
    get_commit_graph_page_impl(&state, branch, cursor, limit, reset)
        .map_err(|err| ErrorPayload::from(&err))
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

fn get_commit_graph_page_impl(
    state: &AppState,
    branch: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    reset: bool,
) -> Result<CommitGraphPageDto, gitsail_domain::GitSailError> {
    let repository = state.repository()?;
    if reset {
        state.reset_commit_graph();
    }

    let branch = branch.map(BranchName::new).transpose()?;
    let query = CommitQuery {
        limit,
        cursor,
        branch,
        ..CommitQuery::default()
    };
    let page = GetCommitHistory::new(state.port()).execute(&repository, &query)?;

    let graph_commits: Vec<GraphCommit> = page.items.iter().map(GraphCommit::from).collect();
    let rows = state.with_commit_graph_mut(|graph| graph.append_page(&graph_commits).to_vec());
    let lane_count = state.with_commit_graph_mut(|graph| graph.lane_count());

    let row_dtos: Vec<CommitGraphRowDto> = rows
        .iter()
        .zip(page.items.iter())
        .map(|(row, commit)| CommitGraphRowDto::from_row_and_commit(row, commit))
        .collect();

    Ok(CommitGraphPageDto {
        rows: row_dtos,
        lane_count: lane_count as u32,
        has_more: page.has_more,
        next_cursor: page.next_cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    use gitsail_application::{BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page};
    use gitsail_domain::{
        Blame, Branch, BranchName, CancellationToken, ChangeType, Commit, CommitHash, ErrorCode,
        FileChange, FileStatusCode, GitSailError, GitTimestamp, HeadState, LineHistory,
        Repository, RepositoryId, RepositoryStatus, Signature,
    };

    /// A `RepositoryReadPort` double exercising `discover`, `status`, and
    /// (for this story) `commits` — every other method is unreachable from
    /// these tests and left `unimplemented!()`, matching the pattern
    /// already used by `gitsail-application/src/session.rs`'s own test
    /// doubles. `history` is newest-first, like a real `git log`; `commits`
    /// paginates it with an offset cursor, the same convention
    /// `gitsail-git`'s real adapter uses.
    struct FakePort {
        repository: Repository,
        status: RepositoryStatus,
        history: Vec<Commit>,
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
            query: &CommitQuery,
        ) -> Result<Page<Commit>, GitSailError> {
            let limit = query.limit.unwrap_or(50) as usize;
            let offset: usize = match &query.cursor {
                None => 0,
                Some(cursor) => cursor
                    .parse()
                    .map_err(|_| GitSailError::new(ErrorCode::ParseFailure, "bad cursor"))?,
            };
            let items: Vec<Commit> = self.history.iter().skip(offset).take(limit).cloned().collect();
            let next_offset = offset + items.len();
            let has_more = next_offset < self.history.len();
            Ok(Page {
                items,
                next_cursor: has_more.then(|| next_offset.to_string()),
                has_more,
            })
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

    fn sample_commit(hash: &str, parents: &[&str], subject: &str) -> Commit {
        let hash = CommitHash::new(hash).unwrap();
        Commit {
            short_hash: hash.to_short(8),
            hash,
            parents: parents.iter().map(|p| CommitHash::new(*p).unwrap()).collect(),
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: subject.to_string(),
            body: String::new(),
            decorations: vec![],
        }
    }

    fn state_with_fake_port() -> AppState {
        state_with_history(vec![])
    }

    fn state_with_history(history: Vec<Commit>) -> AppState {
        let port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(FakePort {
            repository: sample_repository(),
            status: dirty_status(),
            history,
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

    fn linear_history() -> Vec<Commit> {
        let hash_a = "a".repeat(40);
        let hash_b = "b".repeat(40);
        vec![
            sample_commit(&hash_b, &[&hash_a], "second commit"),
            sample_commit(&hash_a, &[], "initial commit"),
        ]
    }

    #[test]
    fn get_commit_graph_page_before_opening_fails_with_invalid_repository_state() {
        let state = state_with_fake_port();

        let err = get_commit_graph_page_impl(&state, None, None, None, false).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn get_commit_graph_page_returns_rows_with_lane_and_resolved_edges() {
        let state = state_with_history(linear_history());
        open_repository_impl(&state, "/repo").unwrap();

        let page = get_commit_graph_page_impl(&state, None, None, Some(10), false).unwrap();

        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.lane_count, 1);
        assert!(!page.has_more);
        assert_eq!(page.rows[0].commit.subject, "second commit");
        assert_eq!(page.rows[0].edges.len(), 1);
        assert!(
            page.rows[0].edges[0].resolved,
            "the parent is in the same page, so the edge must resolve immediately"
        );
        assert!(page.rows[1].edges.is_empty(), "the root commit has no edges");
    }

    #[test]
    fn get_commit_graph_page_pagination_resolves_the_earlier_pages_continuation() {
        let state = state_with_history(linear_history());
        open_repository_impl(&state, "/repo").unwrap();

        let first = get_commit_graph_page_impl(&state, None, None, Some(1), false).unwrap();
        assert_eq!(first.rows.len(), 1);
        assert!(first.has_more);
        assert!(
            !first.rows[0].edges[0].resolved,
            "the parent has not loaded yet"
        );

        let second =
            get_commit_graph_page_impl(&state, None, first.next_cursor, Some(1), false).unwrap();
        assert_eq!(second.rows.len(), 1);
        assert!(!second.has_more);

        // The first page's row is still the same object inside the
        // accumulated graph; its edge must now report resolved.
        let resolved_now = state.with_commit_graph_mut(|graph| graph.rows()[0].edges[0].resolved);
        assert!(
            resolved_now,
            "appending the next page must resolve the earlier page's continuation edge"
        );
    }

    #[test]
    fn reset_true_discards_previously_accumulated_rows() {
        let state = state_with_history(linear_history());
        open_repository_impl(&state, "/repo").unwrap();

        get_commit_graph_page_impl(&state, None, None, Some(10), false).unwrap();
        assert_eq!(state.with_commit_graph_mut(|g| g.rows().len()), 2);

        let page = get_commit_graph_page_impl(&state, None, None, Some(1), true).unwrap();

        assert_eq!(
            page.rows.len(),
            1,
            "a reset call must only return its own page's rows"
        );
        assert_eq!(
            state.with_commit_graph_mut(|g| g.rows().len()),
            1,
            "reset must discard whatever was accumulated by an earlier filter/query"
        );
    }

    #[test]
    fn opening_a_new_repository_also_resets_the_commit_graph() {
        let state = state_with_history(linear_history());
        open_repository_impl(&state, "/repo").unwrap();
        get_commit_graph_page_impl(&state, None, None, Some(10), false).unwrap();
        assert_eq!(state.with_commit_graph_mut(|g| g.rows().len()), 2);

        open_repository_impl(&state, "/repo").unwrap();

        assert_eq!(
            state.with_commit_graph_mut(|g| g.rows().len()),
            0,
            "re-opening a repository must never carry over the previous graph"
        );
    }
}
