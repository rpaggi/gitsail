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
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    CommitQuery, ForgetRecentRepository, GetCommitHistory, ListRecentRepositories, OpenRepository,
    RecordRecentRepository, RefreshReason,
};
use gitsail_domain::{BranchName, ErrorCode, GitSailError, GraphCommit};
use gitsail_protocol::{
    CommitGraphPageDto, CommitGraphRowDto, ErrorPayload, RecentRepositoryDto, RepositoryDto,
    RepositoryStatusDto,
};

use crate::state::AppState;

/// Seconds since the Unix epoch, used to timestamp a recent-repository
/// entry (US-052 criterion 1). A clock read failure (the system clock set
/// before 1970) falls back to `0` rather than panicking or failing the
/// open — recording a recent is a best-effort convenience, never a
/// precondition for opening a repository.
fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Maps the frontend's refresh-trigger string to [`RefreshReason`] (US-054
/// criterion 2: manual, focus, and after-mutation refreshes are all wired
/// through here to the one shared session refresh, rather than each
/// inventing its own path). An unrecognized value defaults to `Manual`
/// rather than failing the refresh over what is purely an observability
/// tag — `RefreshReason` never changes refresh *behavior* (see
/// `gitsail_application::session`).
fn parse_refresh_reason(reason: &str) -> RefreshReason {
    match reason {
        "focus" => RefreshReason::Focus,
        "after_mutation" => RefreshReason::AfterMutation,
        _ => RefreshReason::Manual,
    }
}

#[tauri::command]
pub fn open_repository(
    path: String,
    state: tauri::State<AppState>,
) -> Result<RepositoryDto, ErrorPayload> {
    open_repository_impl(&state, &path).map_err(|err| ErrorPayload::from(&err))
}

#[tauri::command]
pub fn get_repository_status(
    reason: String,
    state: tauri::State<AppState>,
) -> Result<RepositoryStatusDto, ErrorPayload> {
    get_repository_status_impl(&state, &reason).map_err(|err| ErrorPayload::from(&err))
}

/// Lists the recently opened repositories, most-recently-opened first
/// (US-052 criterion 1).
#[tauri::command]
pub fn list_recent_repositories(
    state: tauri::State<AppState>,
) -> Result<Vec<RecentRepositoryDto>, ErrorPayload> {
    list_recent_repositories_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

/// Removes one entry from the recent-repositories list — the explicit
/// confirmation US-052 criterion 2 requires before a moved/inaccessible
/// entry disappears; nothing removes an entry automatically.
#[tauri::command]
pub fn forget_recent_repository(
    path: String,
    state: tauri::State<AppState>,
) -> Result<Vec<RecentRepositoryDto>, ErrorPayload> {
    forget_recent_repository_impl(&state, &path).map_err(|err| ErrorPayload::from(&err))
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

fn open_repository_impl(state: &AppState, path: &str) -> Result<RepositoryDto, GitSailError> {
    let repository = OpenRepository::new(state.port()).execute(Path::new(path))?;
    let dto = RepositoryDto::from(&repository);
    let root_path = repository.root_path.clone();
    state.open_session(repository);
    // Recording a recent repository is a best-effort side effect (US-052
    // criterion 1 is a UX shortcut, not a hard dependency of opening a
    // repository): an unwritable config directory or a corrupted recents
    // file must never turn an otherwise-successful open into a failure.
    let _ = RecordRecentRepository::new(state.recent_repositories())
        .execute(root_path, now_unix_seconds());
    Ok(dto)
}

fn get_repository_status_impl(
    state: &AppState,
    reason: &str,
) -> Result<RepositoryStatusDto, GitSailError> {
    state.with_session_mut(|session| {
        session.refresh(parse_refresh_reason(reason))?;
        Ok(RepositoryStatusDto::from(session.status().expect(
            "status is always Some immediately after a successful refresh",
        )))
    })
}

fn list_recent_repositories_impl(
    state: &AppState,
) -> Result<Vec<RecentRepositoryDto>, GitSailError> {
    let recents = ListRecentRepositories::new(state.recent_repositories()).execute()?;
    Ok(recents.entries().iter().map(RecentRepositoryDto::from).collect())
}

fn forget_recent_repository_impl(
    state: &AppState,
    path: &str,
) -> Result<Vec<RecentRepositoryDto>, GitSailError> {
    let recents =
        ForgetRecentRepository::new(state.recent_repositories()).execute(Path::new(path))?;
    Ok(recents.entries().iter().map(RecentRepositoryDto::from).collect())
}

fn get_commit_graph_page_impl(
    state: &AppState,
    branch: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    reset: bool,
) -> Result<CommitGraphPageDto, GitSailError> {
    // Repository and epoch are captured together (US-054 criterion 3): the
    // `git log` below runs without holding any lock, so a repository
    // switch — and its epoch bump — can freely happen while it is in
    // flight. `epoch` is re-validated right before this page is applied to
    // the shared commit graph, below.
    let (repository, epoch) = state.repository_with_epoch()?;
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
    let (rows, lane_count) = state
        .append_commit_graph_page_if_current(epoch, &graph_commits)
        .ok_or_else(|| {
            GitSailError::new(
                ErrorCode::Cancelled,
                "the repository changed while this page was loading",
            )
        })?;

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
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use gitsail_application::{
        BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page, RecentRepositories,
        RecentRepositoriesPort,
    };
    use gitsail_domain::{
        Blame, Branch, BranchName, CancellationToken, ChangeType, Commit, CommitHash, ErrorCode,
        FileChange, FileStatusCode, GitSailError, GitTimestamp, HeadState, LineHistory,
        Repository, RepositoryId, RepositoryStatus, Signature,
    };

    /// An in-memory [`RecentRepositoriesPort`] double — `commands.rs` tests
    /// care about the use-case wiring, never about disk persistence (that
    /// is `recent_repositories_store`'s job).
    struct InMemoryRecents(Mutex<RecentRepositories>);
    impl InMemoryRecents {
        fn shared() -> Arc<dyn RecentRepositoriesPort> {
            Arc::new(Self(Mutex::new(RecentRepositories::new())))
        }
    }
    impl RecentRepositoriesPort for InMemoryRecents {
        fn load(&self) -> Result<RecentRepositories, GitSailError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, recents: &RecentRepositories) -> Result<(), GitSailError> {
            *self.0.lock().unwrap() = recents.clone();
            Ok(())
        }
    }

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
        AppState::new(port, InMemoryRecents::shared())
    }

    #[test]
    fn open_repository_returns_the_dto_shape_of_the_discovered_repository() {
        let state = state_with_fake_port();

        let dto = open_repository_impl(&state, "/repo").unwrap();

        assert_eq!(dto.root_path, "/repo");
        assert_eq!(dto.current_branch.as_deref(), Some("main"));
    }

    #[test]
    fn open_repository_records_the_canonical_root_path_as_a_recent_repository() {
        let state = state_with_fake_port();

        open_repository_impl(&state, "/repo").unwrap();

        let recents = list_recent_repositories_impl(&state).unwrap();
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].path, "/repo");
    }

    #[test]
    fn forget_recent_repository_removes_the_entry_and_persists_the_removal() {
        let state = state_with_fake_port();
        open_repository_impl(&state, "/repo").unwrap();
        assert_eq!(list_recent_repositories_impl(&state).unwrap().len(), 1);

        let remaining = forget_recent_repository_impl(&state, "/repo").unwrap();

        assert!(remaining.is_empty());
        assert!(list_recent_repositories_impl(&state).unwrap().is_empty());
    }

    #[test]
    fn get_repository_status_before_opening_fails_with_invalid_repository_state() {
        let state = state_with_fake_port();

        let err = get_repository_status_impl(&state, "manual").unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn get_repository_status_after_opening_returns_the_refreshed_status() {
        let state = state_with_fake_port();
        open_repository_impl(&state, "/repo").unwrap();

        let status = get_repository_status_impl(&state, "manual").unwrap();

        assert_eq!(status.files.len(), 1);
    }

    #[test]
    fn get_repository_status_accepts_focus_and_after_mutation_reasons() {
        let state = state_with_fake_port();
        open_repository_impl(&state, "/repo").unwrap();

        assert!(get_repository_status_impl(&state, "focus").is_ok());
        assert!(get_repository_status_impl(&state, "after_mutation").is_ok());
    }

    #[test]
    fn parse_refresh_reason_maps_known_strings_and_defaults_unknown_ones_to_manual() {
        assert_eq!(parse_refresh_reason("focus"), RefreshReason::Focus);
        assert_eq!(parse_refresh_reason("after_mutation"), RefreshReason::AfterMutation);
        assert_eq!(parse_refresh_reason("manual"), RefreshReason::Manual);
        assert_eq!(parse_refresh_reason("something-unknown"), RefreshReason::Manual);
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

    // -- US-052 criterion 3 / US-054 criterion 3: an in-flight read from a
    // superseded session must never leak into whatever repository is now
    // open. `BlockingPort` lets the test control, with real threads and no
    // sleeps, exactly when the "slow git log" resumes relative to the
    // repository switch — the DoD's "artificial delay + repository switch"
    // scenario.

    /// A `RepositoryReadPort` whose `commits()` call announces that it has
    /// started (so the test knows the epoch has already been captured by
    /// the caller) and then blocks until the test releases it.
    struct BlockingPort {
        history: Vec<Commit>,
        started: mpsc::Sender<()>,
        gate: Mutex<mpsc::Receiver<()>>,
    }

    impl gitsail_application::RepositoryReadPort for BlockingPort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn commits(
            &self,
            _repo: &Repository,
            query: &CommitQuery,
        ) -> Result<Page<Commit>, GitSailError> {
            let _ = self.started.send(());
            let _ = self.gate.lock().unwrap().recv();
            let limit = query.limit.unwrap_or(50) as usize;
            let items: Vec<Commit> = self.history.iter().take(limit).cloned().collect();
            Ok(Page { items, next_cursor: None, has_more: false })
        }
        fn commit(&self, _repo: &Repository, _hash: &CommitHash) -> Result<Commit, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn diff(
            &self,
            _repo: &Repository,
            _request: &DiffRequest,
            _cancel: &CancellationToken,
        ) -> Result<gitsail_domain::Diff, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn resolve_revision(
            &self,
            _repo: &Repository,
            _revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn blame(
            &self,
            _repo: &Repository,
            _request: &BlameRequest,
            _cancel: &CancellationToken,
        ) -> Result<Blame, GitSailError> {
            unimplemented!("not exercised by this test")
        }
        fn line_history(
            &self,
            _repo: &Repository,
            _request: &LineHistoryRequest,
            _cancel: &CancellationToken,
        ) -> Result<LineHistory, GitSailError> {
            unimplemented!("not exercised by this test")
        }
    }

    #[test]
    fn a_repository_switch_while_a_commit_graph_read_is_in_flight_never_contaminates_the_new_repository()
    {
        let (started_tx, started_rx) = mpsc::channel();
        let (gate_tx, gate_rx) = mpsc::channel();
        let port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(BlockingPort {
            history: linear_history(),
            started: started_tx,
            gate: Mutex::new(gate_rx),
        });
        let state = Arc::new(AppState::new(port, InMemoryRecents::shared()));
        state.open_session(sample_repository());

        let state_for_thread = Arc::clone(&state);
        let handle = thread::spawn(move || {
            get_commit_graph_page_impl(&state_for_thread, None, None, Some(10), false)
        });

        // Block until the background read has captured its (now current)
        // epoch and reached the (blocked) "git log" call — guaranteed to
        // happen only after `repository_with_epoch()` has already run, by
        // program order inside that thread.
        started_rx.recv().expect("the background read must reach the port call");

        // The repository switch — and its epoch bump — happens while the
        // background read is still blocked "in flight".
        state.open_session(sample_repository());

        // Only now let the stale read finish.
        gate_tx.send(()).expect("the background read must still be waiting on the gate");

        let result = handle.join().expect("the background thread must not panic");

        let err = result.expect_err(
            "a page computed against an epoch the session has since moved past must fail, \
             never silently succeed with stale data",
        );
        assert_eq!(err.code(), ErrorCode::Cancelled);
        assert_eq!(
            state.with_commit_graph_mut(|g| g.rows().len()),
            0,
            "the new session's freshly reset graph must never be contaminated by the old \
             session's stale page"
        );
    }
}
