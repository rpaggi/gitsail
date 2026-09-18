//! Desktop session state (SAD §17's "repository session state" category;
//! §21, §22).
//!
//! [`AppState`] holds exactly one open repository's [`RepositorySession`] at
//! a time, plus a session **epoch** (US-052 criterion 3; US-054 criterion
//! 3): a counter bumped by every [`AppState::open_session`] call, including
//! re-opening the very same path — that is deliberately still treated as a
//! brand new session. Any command that starts a slow read (e.g.
//! `get_commit_graph_page`'s `git log`) captures the epoch active when it
//! started, alongside the repository snapshot, via
//! [`AppState::repository_with_epoch`]; before applying its result to any
//! shared state, it re-validates that epoch (see
//! [`AppState::append_commit_graph_page_if_current`]). A result computed
//! for an epoch that is no longer current — because the user switched
//! repositories while the read was still in flight — is discarded rather
//! than silently corrupting whatever repository is now open. This is the
//! one mechanism both US-052 ("switching a project isolates selection, in
//! flight operations and late results") and US-054 ("stale/cancelled query
//! results never overwrite current data") share.
//!
//! `session` and `commit_graph` remain two separate [`Mutex`]es: every path
//! through this type that ever needs both locks takes `session` first,
//! `commit_graph` second (see
//! [`AppState::append_commit_graph_page_if_current`]), and
//! [`AppState::open_session`] never holds both at once — it fully releases
//! `session`'s lock before taking `commit_graph`'s — so that ordering can
//! never deadlock.
//!
//! Every command that touches the session, including reads, still
//! serializes behind `session`'s lock for the duration of its own port
//! call (e.g. `get_repository_status`'s `git status`). This was a
//! deliberate simplification accepted by US-051 and re-examined for this
//! story (US-054's "keep the GUI responsive"): moving to per-repository or
//! lock-free concurrent reads would let two independent `git` calls run in
//! parallel, but none of US-054's three acceptance criteria require that —
//! the UI thread itself is never blocked (Tauri dispatches every command
//! off it regardless, see `commands.rs`), a visible loading state is a
//! frontend concern, and staleness is already handled by the epoch guard
//! above. A full concurrent-reads redesign is left to a future story if
//! throughput (not correctness or responsiveness) turns out to need it.

use std::sync::{Arc, Mutex};

use gitsail_application::{
    ForgeCredentialPort, PreferencesPort, PullRequestQueryPort, RecentRepositoriesPort,
    RepositoryReadPort, RepositorySession, RepositoryWritePort, UpdateCheckPort,
};
use gitsail_domain::{CommitGraph, ErrorCode, GitSailError, GraphCommit, GraphRow, Repository};

use crate::keybindings_store::JsonFileKeybindingsStore;

/// The Desktop process' parsed startup intent (EPIC-15's Desktop-side gap:
/// see `lib.rs`'s `parse_startup_args`), consumed exactly once by the
/// frontend's first call to `commands::take_startup_intent`. A VS Code
/// handoff (T-210/US-077) launches this process with `--repo <path>
/// --commit <hash>`; a plain launch (double-click, `tauri dev`, ...) leaves
/// both `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupIntent {
    pub repo_path: Option<String>,
    pub commit_hash: Option<String>,
}

/// The active session together with the epoch it was opened at, guarded by
/// one [`Mutex`] so a switch and its epoch bump are always observed
/// together (never a torn read where a caller sees the new repository but
/// the old epoch, or vice versa).
struct SessionSlot {
    session: Option<RepositorySession>,
    epoch: u64,
}

pub struct AppState {
    port: Arc<dyn RepositoryReadPort>,
    /// Mutation capability (EPIC-12: T-191/T-192/T-193's local-branch
    /// subset), kept as a separate field/type from `port` for the same
    /// ADR-009 reason `gitsail-application::write_ports` documents: nothing
    /// that only holds `AppState::port()` gains mutation capability by
    /// accident.
    write_port: Arc<dyn RepositoryWritePort>,
    session: Mutex<SessionSlot>,
    /// The commit graph accumulated for the active repository (US-067),
    /// separate from `session`: a session tracks HEAD/status/selection
    /// (SAD §21), while this is presentation-facing paginated layout state
    /// that only `get_commit_graph_page` touches. Reset whenever a new
    /// repository is opened, or explicitly when a filter change means the
    /// previously accumulated lanes no longer apply (US-067 criterion 3).
    commit_graph: Mutex<CommitGraph>,
    /// Persists the recently-opened-repositories list (US-052). Desktop's
    /// concrete adapter (a JSON file under the OS config directory) lives
    /// in `recent_repositories_store`; `AppState` only depends on the
    /// port, matching every other `gitsail-application` abstraction it
    /// holds.
    recent_repositories: Arc<dyn RecentRepositoriesPort>,
    /// Stores/retrieves forge (GitHub/GitLab) access tokens in OS-secure
    /// storage (T-244/US-102). Desktop's concrete adapter (`keyring`-backed)
    /// lives in `gitsail-forge`; `AppState` only depends on the port,
    /// matching [`Self::recent_repositories`]'s own pattern.
    forge_credentials: Arc<dyn ForgeCredentialPort>,
    /// Queries GitHub/GitLab for PR/MR listing (T-245/US-103). Desktop's
    /// concrete adapter (`gitsail-forge`'s `CompositePullRequestQueryPort`,
    /// dispatching to a real HTTP-backed adapter per forge) lives outside
    /// this crate, matching every other port `AppState` holds.
    pull_request_query: Arc<dyn PullRequestQueryPort>,
    /// Persists GitSail's own local UI preferences — theme, as of T-248/
    /// US-106 (Desktop's concrete adapter, a JSON file, lives in
    /// `preferences_store`; `AppState` only depends on the port, matching
    /// every other `gitsail-application` abstraction it holds).
    preferences: Arc<dyn PreferencesPort>,
    /// Persists custom keyboard shortcut overrides (T-249/US-107). A
    /// concrete type, not a `dyn` port: see `keybindings_store`'s own doc
    /// comment for why this is deliberately a Desktop-only concern with no
    /// `gitsail-application` abstraction in front of it.
    keybindings: Arc<JsonFileKeybindingsStore>,
    /// Queries GitHub for the latest published release (T-260/US-127).
    /// Desktop's concrete adapter (`gitsail-forge`'s
    /// `GitHubReleaseUpdateAdapter`) lives outside this crate, matching
    /// every other port `AppState` holds.
    update_check: Arc<dyn UpdateCheckPort>,
    /// The startup intent parsed from `--repo`/`--commit` (see this
    /// module's own `StartupIntent` doc). `take` semantics (via
    /// `Mutex<Option<_>>`) rather than a plain field: consumed exactly
    /// once, by whichever frontend call reads it first — a second read (a
    /// stray re-render, a reload) must never re-open/re-select the same
    /// startup target a second time behind the person's back.
    startup_intent: Mutex<Option<StartupIntent>>,
}

impl AppState {
    // Every argument here is a distinct port/adapter `AppState` composes
    // (SAD §17: Tauri commands stay thin, `AppState` is where every
    // capability the frontend needs gets wired together once, in
    // `lib.rs::run`) — the same reason this constructor has grown one
    // parameter per story since T-184/US-051 first introduced it (T-244,
    // T-245, T-247, T-249, and now T-260's `update_check`). A builder would
    // reduce this count but is not worth introducing for one more `Arc`
    // clone; revisit if this keeps growing.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        port: Arc<dyn RepositoryReadPort>,
        write_port: Arc<dyn RepositoryWritePort>,
        recent_repositories: Arc<dyn RecentRepositoriesPort>,
        forge_credentials: Arc<dyn ForgeCredentialPort>,
        pull_request_query: Arc<dyn PullRequestQueryPort>,
        preferences: Arc<dyn PreferencesPort>,
        keybindings: Arc<JsonFileKeybindingsStore>,
        update_check: Arc<dyn UpdateCheckPort>,
    ) -> Self {
        Self {
            port,
            write_port,
            session: Mutex::new(SessionSlot {
                session: None,
                epoch: 0,
            }),
            commit_graph: Mutex::new(CommitGraph::new()),
            recent_repositories,
            forge_credentials,
            pull_request_query,
            preferences,
            keybindings,
            update_check,
            startup_intent: Mutex::new(None),
        }
    }

    pub fn port(&self) -> Arc<dyn RepositoryReadPort> {
        self.port.clone()
    }

    pub fn write_port(&self) -> Arc<dyn RepositoryWritePort> {
        self.write_port.clone()
    }

    pub fn recent_repositories(&self) -> Arc<dyn RecentRepositoriesPort> {
        self.recent_repositories.clone()
    }

    pub fn forge_credentials(&self) -> Arc<dyn ForgeCredentialPort> {
        self.forge_credentials.clone()
    }

    pub fn pull_request_query(&self) -> Arc<dyn PullRequestQueryPort> {
        self.pull_request_query.clone()
    }

    pub fn preferences(&self) -> Arc<dyn PreferencesPort> {
        self.preferences.clone()
    }

    pub fn keybindings(&self) -> Arc<JsonFileKeybindingsStore> {
        self.keybindings.clone()
    }

    pub fn update_check(&self) -> Arc<dyn UpdateCheckPort> {
        self.update_check.clone()
    }

    /// Records the startup intent parsed from argv (`lib.rs::run`, once,
    /// before the Tauri event loop starts).
    pub fn set_startup_intent(&self, intent: StartupIntent) {
        *self
            .startup_intent
            .lock()
            .expect("startup intent mutex poisoned") = Some(intent);
    }

    /// Consumes and returns the startup intent, leaving `None` behind for
    /// any later call — see [`StartupIntent`]'s own doc for why this is
    /// "take", not "get".
    pub fn take_startup_intent(&self) -> StartupIntent {
        self.startup_intent
            .lock()
            .expect("startup intent mutex poisoned")
            .take()
            .unwrap_or_default()
    }

    /// Replaces the active session with a fresh one over `repository`,
    /// discarding any previously open repository's session state, bumps
    /// the session epoch, and starts a brand new commit graph (a graph
    /// accumulated for the previous repository must never be appended to
    /// as if it were the new one's history). Returns the new epoch.
    pub fn open_session(&self, repository: Repository) -> u64 {
        let session = RepositorySession::new(self.port.clone(), repository);
        let epoch = {
            let mut guard = self.session.lock().expect("session mutex poisoned");
            guard.session = Some(session);
            guard.epoch += 1;
            guard.epoch
        };
        self.reset_commit_graph();
        epoch
    }

    /// Runs `f` against the active session, or fails with
    /// [`ErrorCode::InvalidRepositoryState`] when no repository is open yet
    /// — the same error taxonomy every other GitSail surface uses, rather
    /// than a Desktop-specific error shape (US-051 criterion 3).
    pub fn with_session_mut<T>(
        &self,
        f: impl FnOnce(&mut RepositorySession) -> Result<T, GitSailError>,
    ) -> Result<T, GitSailError> {
        let mut guard = self.session.lock().expect("session mutex poisoned");
        match guard.session.as_mut() {
            Some(session) => f(session),
            None => Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no repository is open; call open_repository first",
            )),
        }
    }

    /// The active repository together with the session epoch active at the
    /// moment of this read, or the same
    /// [`ErrorCode::InvalidRepositoryState`] failure
    /// [`Self::with_session_mut`] uses. A caller doing slow work outside
    /// any lock (a `git log`, a diff, ...) must carry the epoch forward
    /// and re-validate it before mutating shared state with the result —
    /// see [`Self::append_commit_graph_page_if_current`].
    pub fn repository_with_epoch(&self) -> Result<(Repository, u64), GitSailError> {
        let guard = self.session.lock().expect("session mutex poisoned");
        let session = guard.session.as_ref().ok_or_else(|| {
            GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no repository is open; call open_repository first",
            )
        })?;
        Ok((session.repository().clone(), guard.epoch))
    }

    /// The session's current epoch, independent of whether a repository is
    /// open (epoch `0` before the first [`Self::open_session`] call).
    /// Test-only: production code always carries an epoch forward from
    /// [`Self::repository_with_epoch`]/[`Self::open_session`] rather than
    /// reading it independently.
    #[cfg(test)]
    pub fn current_epoch(&self) -> u64 {
        self.session.lock().expect("session mutex poisoned").epoch
    }

    /// Discards the accumulated commit graph, starting the next
    /// `get_commit_graph_page` call from an empty graph (US-067 criterion
    /// 3: a filter change gets a fresh layout rather than one mixing rows
    /// from two different queries).
    pub fn reset_commit_graph(&self) {
        *self
            .commit_graph
            .lock()
            .expect("commit graph mutex poisoned") = CommitGraph::new();
    }

    /// Runs `f` against the accumulated commit graph. Test-only:
    /// production code only ever reads the graph through
    /// [`Self::append_commit_graph_page_if_current`], which folds the
    /// epoch check into the same access.
    #[cfg(test)]
    pub fn with_commit_graph_mut<T>(&self, f: impl FnOnce(&mut CommitGraph) -> T) -> T {
        let mut guard = self
            .commit_graph
            .lock()
            .expect("commit graph mutex poisoned");
        f(&mut guard)
    }

    /// Appends `commits` to the accumulated commit graph, but only if
    /// `epoch` still matches the session's current epoch (US-054 criterion
    /// 3) — otherwise a page computed for a repository the caller has
    /// since switched away from would silently corrupt the *new*
    /// repository's graph. Returns `None` when `epoch` is stale; the caller
    /// (`commands::get_commit_graph_page`) turns that into an
    /// [`ErrorCode::Cancelled`] error rather than a page of wrong data.
    ///
    /// Locks `session` before `commit_graph`, the same order every other
    /// path through this type uses, so this can never deadlock against
    /// [`Self::open_session`] (which never holds both locks at once).
    pub fn append_commit_graph_page_if_current(
        &self,
        epoch: u64,
        commits: &[GraphCommit],
    ) -> Option<(Vec<GraphRow>, usize)> {
        let session_guard = self.session.lock().expect("session mutex poisoned");
        if session_guard.epoch != epoch {
            return None;
        }
        let mut graph_guard = self
            .commit_graph
            .lock()
            .expect("commit graph mutex poisoned");
        let rows = graph_guard.append_page(commits).to_vec();
        let lane_count = graph_guard.lane_count();
        Some((rows, lane_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_application::{RecentRepositories, RecentRepositoriesPort};
    use gitsail_domain::{BranchName, CommitHash, HeadState, RepositoryId};
    use std::path::{Path, PathBuf};

    struct UnimplementedPort;
    impl RepositoryReadPort for UnimplementedPort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            unimplemented!()
        }
        fn status(
            &self,
            _repo: &Repository,
        ) -> Result<gitsail_domain::RepositoryStatus, GitSailError> {
            unimplemented!()
        }
        fn commits(
            &self,
            _repo: &Repository,
            _query: &gitsail_application::CommitQuery,
        ) -> Result<gitsail_application::Page<gitsail_domain::Commit>, GitSailError> {
            unimplemented!()
        }
        fn commit(
            &self,
            _repo: &Repository,
            _hash: &CommitHash,
        ) -> Result<gitsail_domain::Commit, GitSailError> {
            unimplemented!()
        }
        fn branches(
            &self,
            _repo: &Repository,
        ) -> Result<Vec<gitsail_domain::Branch>, GitSailError> {
            unimplemented!()
        }
        fn diff(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::DiffRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::Diff, GitSailError> {
            unimplemented!()
        }
        fn resolve_revision(
            &self,
            _repo: &Repository,
            _revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!()
        }
        fn blame(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::BlameRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::Blame, GitSailError> {
            unimplemented!()
        }
        fn line_history(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::LineHistoryRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::LineHistory, GitSailError> {
            unimplemented!()
        }
        fn file_content(
            &self,
            _repo: &Repository,
            _revision: &CommitHash,
            _path: &Path,
        ) -> Result<gitsail_domain::FileContentAtRevision, GitSailError> {
            unimplemented!()
        }
    }

    struct UnimplementedWritePort;
    impl RepositoryWritePort for UnimplementedWritePort {
        fn stage_files(&self, _repo: &Repository, _paths: &[PathBuf]) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn unstage_files(
            &self,
            _repo: &Repository,
            _paths: &[PathBuf],
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn create_commit(
            &self,
            _repo: &Repository,
            _message: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!()
        }
        fn stage_hunks(
            &self,
            _repo: &Repository,
            _selection: &[gitsail_domain::FileDiff],
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn unstage_hunks(
            &self,
            _repo: &Repository,
            _selection: &[gitsail_domain::FileDiff],
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn switch_branch(
            &self,
            _repo: &Repository,
            _target: &BranchName,
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn create_branch(
            &self,
            _repo: &Repository,
            _name: &BranchName,
            _start_point: Option<&CommitHash>,
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn delete_branch(
            &self,
            _repo: &Repository,
            _name: &BranchName,
            _force: bool,
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn rename_branch(
            &self,
            _repo: &Repository,
            _old_name: &BranchName,
            _new_name: &BranchName,
        ) -> Result<(), GitSailError> {
            unimplemented!()
        }
        fn amend_commit(
            &self,
            _repo: &Repository,
            _message: &str,
            _expected_head: &CommitHash,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!()
        }
    }

    /// An in-memory [`RecentRepositoriesPort`] double: `AppState`'s own
    /// tests care about session/epoch/commit-graph behavior, never about
    /// how recents are persisted (that is `recent_repositories_store`'s
    /// job), so this never touches disk.
    struct InMemoryRecents(Mutex<RecentRepositories>);
    impl InMemoryRecents {
        fn new() -> Self {
            Self(Mutex::new(RecentRepositories::new()))
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

    /// An in-memory [`PreferencesPort`] double, mirroring
    /// `gitsail_application::preferences`'s own test double — `AppState`'s
    /// tests care about session/epoch/commit-graph behavior, never about
    /// how preferences are persisted (that is `preferences_store`'s job).
    struct InMemoryPreferences(Mutex<Option<gitsail_application::Preferences>>);
    impl InMemoryPreferences {
        fn new() -> Self {
            Self(Mutex::new(None))
        }
    }
    impl PreferencesPort for InMemoryPreferences {
        fn load(&self) -> Result<gitsail_application::PreferencesLoadOutcome, GitSailError> {
            let stored = self.0.lock().unwrap().clone().unwrap_or_default();
            Ok(gitsail_application::PreferencesLoadOutcome::clean(stored))
        }
        fn save(&self, preferences: &gitsail_application::Preferences) -> Result<(), GitSailError> {
            *self.0.lock().unwrap() = Some(preferences.clone());
            Ok(())
        }
    }

    fn state() -> AppState {
        AppState::new(
            Arc::new(UnimplementedPort),
            Arc::new(UnimplementedWritePort),
            Arc::new(InMemoryRecents::new()),
            Arc::new(gitsail_forge::InMemoryForgeCredentialStore::new()),
            Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
            Arc::new(InMemoryPreferences::new()),
            Arc::new(JsonFileKeybindingsStore::new(std::env::temp_dir().join(
                format!(
                    "gitsail-state-test-keybindings-{}-{:?}.json",
                    std::process::id(),
                    std::thread::current().id()
                ),
            ))),
            Arc::new(gitsail_forge::FakeUpdateCheckPort::default()),
        )
    }

    fn sample_repository(root: &str) -> Repository {
        Repository {
            id: RepositoryId::from_canonical_root(Path::new(root)),
            root_path: PathBuf::from(root),
            worktree_path: Some(PathBuf::from(root)),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        }
    }

    fn sample_graph_commit(hash: &str) -> GraphCommit {
        GraphCommit {
            hash: CommitHash::new(hash).unwrap(),
            parents: vec![],
            decorations: vec![],
        }
    }

    #[test]
    fn no_repository_open_reports_invalid_repository_state_and_epoch_zero() {
        let state = state();

        assert_eq!(state.current_epoch(), 0);
        let err = state.repository_with_epoch().unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn opening_a_repository_bumps_the_epoch_every_time_including_the_same_path() {
        let state = state();

        let epoch1 = state.open_session(sample_repository("/repo"));
        let epoch2 = state.open_session(sample_repository("/repo"));

        assert_eq!(epoch1, 1);
        assert_eq!(
            epoch2, 2,
            "re-opening the same path must still start a brand new session/epoch"
        );
        assert_eq!(state.current_epoch(), 2);
    }

    #[test]
    fn repository_with_epoch_reports_the_epoch_active_at_the_time_of_the_read() {
        let state = state();
        let epoch = state.open_session(sample_repository("/repo"));

        let (repository, read_epoch) = state.repository_with_epoch().unwrap();

        assert_eq!(repository.root_path, PathBuf::from("/repo"));
        assert_eq!(read_epoch, epoch);
    }

    #[test]
    fn append_commit_graph_page_if_current_succeeds_for_the_current_epoch() {
        let state = state();
        let epoch = state.open_session(sample_repository("/repo"));

        let (rows, lane_count) = state
            .append_commit_graph_page_if_current(epoch, &[sample_graph_commit(&"a".repeat(40))])
            .expect("the epoch just opened must still be current");

        assert_eq!(rows.len(), 1);
        assert_eq!(lane_count, 1);
        assert_eq!(state.with_commit_graph_mut(|g| g.rows().len()), 1);
    }

    #[test]
    fn append_commit_graph_page_if_current_discards_a_stale_epoch_without_mutating_the_graph() {
        let state = state();
        let stale_epoch = state.open_session(sample_repository("/repo-a"));

        // Simulates a slow read for repo-a that is still in flight when the
        // user switches to repo-b: the switch happens first...
        state.open_session(sample_repository("/repo-b"));
        // ...then repo-a's now-stale read finally tries to append its page.
        let result = state.append_commit_graph_page_if_current(
            stale_epoch,
            &[sample_graph_commit(&"a".repeat(40))],
        );

        assert!(
            result.is_none(),
            "a stale epoch must never be allowed to append"
        );
        assert_eq!(
            state.with_commit_graph_mut(|g| g.rows().len()),
            0,
            "repo-b's freshly reset graph must never be contaminated by repo-a's stale page"
        );
    }

    #[test]
    fn recent_repositories_port_is_reachable_from_state() {
        let state = state();
        let port = state.recent_repositories();

        port.save(&{
            let mut r = RecentRepositories::new();
            r.touch(PathBuf::from("/repo"), 1);
            r
        })
        .unwrap();

        assert_eq!(port.load().unwrap().entries().len(), 1);
    }

    #[test]
    fn take_startup_intent_defaults_to_empty_when_nothing_was_set() {
        let state = state();

        let intent = state.take_startup_intent();

        assert_eq!(intent, StartupIntent::default());
    }

    #[test]
    fn take_startup_intent_returns_the_set_value_exactly_once() {
        let state = state();
        state.set_startup_intent(StartupIntent {
            repo_path: Some("/repo".to_string()),
            commit_hash: Some("a".repeat(40)),
        });

        let first = state.take_startup_intent();
        let second = state.take_startup_intent();

        assert_eq!(first.repo_path.as_deref(), Some("/repo"));
        assert_eq!(first.commit_hash.as_deref(), Some("a".repeat(40).as_str()));
        assert_eq!(
            second,
            StartupIntent::default(),
            "a startup intent must be consumed exactly once, never re-applied on a later read"
        );
    }
}
