//! Application state and the "Update" stage of SAD §18's event/update/
//! render model (US-040, US-041).
//!
//! [`App`] holds a [`RepositorySession`] (SAD §21) — the first real caller
//! of that type outside its own unit tests; the CLI (EPIC-08) uses the
//! read use cases directly and stays stateless between invocations, so a
//! session had no consumer until now. [`App::update`] is a pure function
//! from `(&mut App, Action)` to the [`Command`]s it wants run in the
//! background: it never touches a [`RepositoryReadPort`] or spawns a
//! thread itself, which is what makes "operação lenta não trava
//! navegação" (US-041) and "descarte de resultado antigo" testable without
//! a terminal or real timing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitsail_application::{RefreshReason, RepositoryReadPort, RepositorySession};
use gitsail_domain::{Branch, GitSailError, HeadState, Repository, RepositoryStatus};

use crate::action::Action;
use crate::keymap::InputContext;
use crate::operation::OperationState;
use crate::worker::Command;

/// One of the five regions US-040 criterion 1 requires ("sidebar, graph,
/// detalhes, diff e barra de atalhos"). The shortcuts bar is not a focus
/// target — it has nothing to navigate — so it is not a [`Panel`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Sidebar,
    Graph,
    Details,
    Diff,
}

impl Panel {
    const ALL: [Panel; 4] = [Panel::Sidebar, Panel::Graph, Panel::Details, Panel::Diff];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Sidebar => "Sidebar",
            Panel::Graph => "Graph",
            Panel::Details => "Details",
            Panel::Diff => "Diff",
        }
    }

    /// Cycles to the next panel in a fixed, documented order (US-042
    /// criterion 2: focus is movable between panels without a mouse).
    pub fn next(self) -> Panel {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap();
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Panel {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap();
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// The four states US-040 criterion 2 requires stay visible: "Repositório/
/// branch e estados de carregamento, vazio e erro ficam visíveis". Carries
/// no data itself — [`App::status_error`]/[`App::discovery_error`]/
/// [`App::session`] hold the detail — so it stays cheap to compute and
/// compare in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPhase {
    Loading,
    Loaded,
    /// A freshly initialized repository with no commits yet
    /// ([`HeadState::Unborn`]) and nothing staged.
    Empty,
    Error,
}

/// Application state (the "App State" box in SAD §18's diagram).
pub struct App {
    repo_path: PathBuf,
    port: Arc<dyn RepositoryReadPort>,
    session: Option<RepositorySession>,
    discovery_error: Option<GitSailError>,
    status_error: Option<GitSailError>,
    branches: Vec<Branch>,
    focus: Panel,
    sidebar_cursor: usize,
    help_visible: bool,
    search: Option<String>,
    operation: OperationState,
    should_quit: bool,
    low_color: bool,
    frame_size: (u16, u16),
}

impl App {
    /// Builds the initial state and the [`Command`]s needed to populate it
    /// — opening `repo_path` is itself dispatched as a background command
    /// rather than run inline, so the very first frame already renders the
    /// `Loading` phase (US-040 criterion 2) instead of blocking before the
    /// terminal shows anything.
    pub fn new(
        repo_path: PathBuf,
        port: Arc<dyn RepositoryReadPort>,
        low_color: bool,
    ) -> (App, Vec<Command>) {
        let app = App {
            repo_path: repo_path.clone(),
            port,
            session: None,
            discovery_error: None,
            status_error: None,
            branches: Vec::new(),
            focus: Panel::Sidebar,
            sidebar_cursor: 0,
            help_visible: false,
            search: None,
            operation: OperationState::default(),
            should_quit: false,
            low_color,
            frame_size: (0, 0),
        };
        (app, vec![Command::OpenRepository(repo_path)])
    }

    // -- Accessors used by `ui` and tests ----------------------------------

    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    pub fn session(&self) -> Option<&RepositorySession> {
        self.session.as_ref()
    }

    pub fn discovery_error(&self) -> Option<&GitSailError> {
        self.discovery_error.as_ref()
    }

    pub fn status_error(&self) -> Option<&GitSailError> {
        self.status_error.as_ref()
    }

    pub fn focus(&self) -> Panel {
        self.focus
    }

    pub fn sidebar_cursor(&self) -> usize {
        self.sidebar_cursor
    }

    pub fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn search(&self) -> Option<&str> {
        self.search.as_deref()
    }

    pub fn operation(&self) -> &OperationState {
        &self.operation
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn low_color(&self) -> bool {
        self.low_color
    }

    pub fn frame_size(&self) -> (u16, u16) {
        self.frame_size
    }

    /// Branches matching the active search filter (case-insensitive
    /// substring on the branch name), or every branch when no filter is
    /// active.
    pub fn filtered_branches(&self) -> Vec<&Branch> {
        match self.search.as_deref() {
            Some(query) if !query.is_empty() => {
                let query = query.to_lowercase();
                self.branches
                    .iter()
                    .filter(|b| b.name.as_str().to_lowercase().contains(&query))
                    .collect()
            }
            _ => self.branches.iter().collect(),
        }
    }

    /// The combined view phase US-040 criterion 2 requires stay visible.
    pub fn view_phase(&self) -> ViewPhase {
        if self.discovery_error.is_some() || self.status_error.is_some() {
            return ViewPhase::Error;
        }
        let Some(session) = &self.session else {
            return ViewPhase::Loading;
        };
        match session.status() {
            None => ViewPhase::Loading,
            Some(status) => {
                if matches!(status.head_state, HeadState::Unborn) && status.is_clean() {
                    ViewPhase::Empty
                } else {
                    ViewPhase::Loaded
                }
            }
        }
    }

    /// Which keys are currently meaningful (US-042 criterion 3: help/
    /// search never let a hidden action through).
    pub fn input_context(&self) -> InputContext {
        if self.help_visible {
            InputContext::Help
        } else if self.search.is_some() {
            InputContext::Search
        } else {
            InputContext::Normal
        }
    }

    pub fn handle_resize(&mut self, width: u16, height: u16) {
        self.frame_size = (width, height);
    }

    // -- Update -------------------------------------------------------------

    /// Applies `action`, returning any [`Command`]s it triggers. Never
    /// blocks and never touches `self.port` beyond cloning the `Arc` into a
    /// `Command` for the caller to dispatch (US-041 criterion 2).
    pub fn update(&mut self, action: Action) -> Vec<Command> {
        match action {
            Action::FocusNext => {
                self.focus = self.focus.next();
                Vec::new()
            }
            Action::FocusPrev => {
                self.focus = self.focus.prev();
                Vec::new()
            }
            Action::MoveUp => {
                self.move_cursor(-1);
                Vec::new()
            }
            Action::MoveDown => {
                self.move_cursor(1);
                Vec::new()
            }
            Action::Activate => {
                self.activate();
                Vec::new()
            }
            Action::Dismiss => {
                self.dismiss();
                Vec::new()
            }
            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
                Vec::new()
            }
            Action::StartSearch => {
                self.search = Some(String::new());
                self.sidebar_cursor = 0;
                Vec::new()
            }
            Action::SearchInput(c) => {
                if let Some(query) = self.search.as_mut() {
                    query.push(c);
                }
                self.sidebar_cursor = 0;
                Vec::new()
            }
            Action::SearchBackspace => {
                if let Some(query) = self.search.as_mut() {
                    query.pop();
                }
                Vec::new()
            }
            Action::Refresh => self.refresh_commands(),
            Action::Quit => {
                self.should_quit = true;
                Vec::new()
            }
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.filtered_branches().len();
        if len == 0 {
            self.sidebar_cursor = 0;
            return;
        }
        let current = self.sidebar_cursor as i32;
        let next = (current + delta).rem_euclid(len as i32);
        self.sidebar_cursor = next as usize;
    }

    fn activate(&mut self) {
        if self.focus != Panel::Sidebar {
            return;
        }
        let Some(branch) = self
            .filtered_branches()
            .get(self.sidebar_cursor)
            .map(|b| b.name.clone())
        else {
            return;
        };
        if let Some(session) = self.session.as_mut() {
            session.select_branch(branch);
        }
    }

    fn dismiss(&mut self) {
        if self.help_visible {
            self.help_visible = false;
        } else if self.search.is_some() {
            self.search = None;
            self.sidebar_cursor = 0;
        } else {
            self.operation.cancel();
        }
    }

    fn refresh_commands(&mut self) -> Vec<Command> {
        let Some(session) = self.session.as_mut() else {
            return Vec::new();
        };
        let ticket = session.begin_refresh(RefreshReason::Manual);
        let repo = session.repository().clone();
        vec![
            Command::RefreshStatus(ticket, repo.clone()),
            Command::LoadBranches(session.generation(), repo),
        ]
    }

    // -- Background results ---------------------------------------------

    /// Handles [`crate::message::Message::RepositoryOpened`]: on success,
    /// creates the session and immediately requests its first status/
    /// branches load; on failure, records a user-visible error without
    /// ever constructing a session (US-040 criterion 2's error state).
    pub fn on_repository_opened(
        &mut self,
        result: Result<Repository, GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(repository) => {
                let mut session = RepositorySession::new(Arc::clone(&self.port), repository);
                let ticket = session.begin_refresh(RefreshReason::Manual);
                let repo = session.repository().clone();
                let generation = session.generation();
                self.session = Some(session);
                self.discovery_error = None;
                vec![
                    Command::RefreshStatus(ticket, repo.clone()),
                    Command::LoadBranches(generation, repo),
                ]
            }
            Err(error) => {
                self.discovery_error = Some(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::StatusRefreshed`]. A `ticket`
    /// from before the session's current generation — a newer refresh
    /// already started, or the session was replaced — is discarded
    /// silently, exactly for a failure as
    /// [`gitsail_application::RepositorySession::apply_refresh`] already
    /// does for a success (US-041 criterion 3).
    pub fn on_status_refreshed(
        &mut self,
        ticket: gitsail_application::RefreshTicket,
        result: Result<RepositoryStatus, GitSailError>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.is_current(ticket) {
            return;
        }
        match result {
            Ok(status) => {
                session.apply_refresh(ticket, status);
                self.status_error = None;
            }
            Err(error) => {
                self.status_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::BranchesLoaded`], discarding a
    /// result computed for a generation the session has since moved past
    /// (US-041 criterion 3).
    pub fn on_branches_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<Branch>, GitSailError>,
    ) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        if let Ok(branches) = result {
            self.branches = branches;
            let max = self.filtered_branches().len().saturating_sub(1);
            self.sidebar_cursor = self.sidebar_cursor.min(max);
        }
        // A branch-load failure is left to the sidebar's existing (possibly
        // stale) list rather than promoting a secondary panel's error into
        // the whole view's error phase — the repository/status themselves
        // are what US-040 criterion 2 requires an error state for.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        BranchKind, BranchName, ChangeType, CommitHash, ErrorCode, FileChange, FileStatusCode,
        RepositoryId,
    };
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    /// A double whose `status` call blocks until the test releases it, so
    /// tests can assert the render loop stays responsive while a "slow"
    /// Git operation is outstanding (US-041 DoD: "Teste com operação
    /// deliberadamente lenta comprova navegação e descarte de resultado
    /// antigo").
    struct FakePort {
        repository: Repository,
        status: Mutex<RepositoryStatus>,
        branches: Vec<Branch>,
        status_gate: Option<Mutex<mpsc::Receiver<()>>>,
    }

    impl RepositoryReadPort for FakePort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            Ok(self.repository.clone())
        }

        fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
            if let Some(gate) = &self.status_gate {
                let _ = gate.lock().unwrap().recv();
            }
            Ok(self.status.lock().unwrap().clone())
        }

        fn commits(
            &self,
            _repo: &Repository,
            _query: &gitsail_application::CommitQuery,
        ) -> Result<gitsail_application::Page<gitsail_domain::Commit>, GitSailError> {
            unimplemented!("not exercised by app tests")
        }

        fn commit(
            &self,
            _repo: &Repository,
            _hash: &CommitHash,
        ) -> Result<gitsail_domain::Commit, GitSailError> {
            unimplemented!("not exercised by app tests")
        }

        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            Ok(self.branches.clone())
        }

        fn diff(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::DiffRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::Diff, GitSailError> {
            unimplemented!("not exercised by app tests")
        }

        fn resolve_revision(
            &self,
            _repo: &Repository,
            _revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by app tests")
        }

        fn blame(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::BlameRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::Blame, GitSailError> {
            unimplemented!("not exercised by app tests")
        }

        fn line_history(
            &self,
            _repo: &Repository,
            _request: &gitsail_application::LineHistoryRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::LineHistory, GitSailError> {
            unimplemented!("not exercised by app tests")
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

    fn clean_status() -> RepositoryStatus {
        RepositoryStatus {
            branch: Some(BranchName::new("main").unwrap()),
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            files: vec![],
        }
    }

    fn dirty_status() -> RepositoryStatus {
        RepositoryStatus {
            branch: Some(BranchName::new("main").unwrap()),
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            files: vec![FileChange {
                path: PathBuf::from("a.txt"),
                previous_path: None,
                change_type: ChangeType::Modified,
                index_status: FileStatusCode::Unmodified,
                worktree_status: FileStatusCode::Modified,
            }],
        }
    }

    fn sample_branch(name: &str, current: bool) -> Branch {
        Branch {
            name: BranchName::new(name).unwrap(),
            kind: BranchKind::Local,
            target: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            upstream: None,
            ahead: 0,
            behind: 0,
            is_current: current,
        }
    }

    fn port_without_gate() -> Arc<FakePort> {
        Arc::new(FakePort {
            repository: sample_repository(),
            status: Mutex::new(clean_status()),
            branches: vec![sample_branch("main", true), sample_branch("develop", false)],
            status_gate: None,
        })
    }

    fn new_app() -> (App, Arc<FakePort>) {
        let port = port_without_gate();
        let (app, commands) = App::new(PathBuf::from("/repo"), port.clone() as _, false);
        assert!(matches!(commands.as_slice(), [Command::OpenRepository(_)]));
        (app, port)
    }

    #[test]
    fn a_fresh_app_starts_in_the_loading_phase() {
        let (app, _port) = new_app();
        assert_eq!(app.view_phase(), ViewPhase::Loading);
    }

    #[test]
    fn opening_and_refreshing_reaches_the_loaded_phase() {
        let (mut app, _port) = new_app();

        let repo = sample_repository();
        let open_commands = app.on_repository_opened(Ok(repo));
        assert_eq!(app.view_phase(), ViewPhase::Loading);

        let (ticket, repo_for_status) = match open_commands.as_slice() {
            [Command::RefreshStatus(t, r), Command::LoadBranches(_, _)] => (*t, r.clone()),
            other => panic!("unexpected commands: {other:?}"),
        };

        app.on_status_refreshed(ticket, Ok(clean_status()));
        assert_eq!(app.view_phase(), ViewPhase::Loaded);
        assert_eq!(app.session().unwrap().repository(), &repo_for_status);
    }

    #[test]
    fn a_repository_with_no_commits_and_no_changes_is_the_empty_phase() {
        let (mut app, _port) = new_app();
        let mut repo = sample_repository();
        repo.head_state = HeadState::Unborn;
        repo.current_branch = None;
        let commands = app.on_repository_opened(Ok(repo));
        let ticket = match commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };

        app.on_status_refreshed(
            ticket,
            Ok(RepositoryStatus {
                branch: None,
                head_state: HeadState::Unborn,
                files: vec![],
            }),
        );

        assert_eq!(app.view_phase(), ViewPhase::Empty);
    }

    #[test]
    fn a_discovery_failure_is_the_error_phase_and_never_creates_a_session() {
        let (mut app, _port) = new_app();

        app.on_repository_opened(Err(GitSailError::new(
            ErrorCode::RepositoryNotFound,
            "not a git repository",
        )));

        assert_eq!(app.view_phase(), ViewPhase::Error);
        assert!(app.session().is_none());
        assert!(app.discovery_error().is_some());
    }

    #[test]
    fn a_stale_status_result_is_discarded_like_apply_refresh_already_guarantees() {
        let (mut app, _port) = new_app();
        let open_commands = app.on_repository_opened(Ok(sample_repository()));
        let first_ticket = match open_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };

        // A second, newer refresh starts (e.g. the user pressed `r`) before
        // the first one's result arrives.
        let newer_commands = app.update(Action::Refresh);
        let newer_ticket = match newer_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };
        app.on_status_refreshed(newer_ticket, Ok(dirty_status()));
        assert_eq!(app.view_phase(), ViewPhase::Loaded);

        // The stale first result now arrives late.
        app.on_status_refreshed(first_ticket, Ok(clean_status()));

        assert!(
            !app.session().unwrap().status().unwrap().is_clean(),
            "a stale refresh result must never overwrite the newer applied state"
        );
    }

    #[test]
    fn a_stale_status_error_is_also_discarded() {
        let (mut app, _port) = new_app();
        let open_commands = app.on_repository_opened(Ok(sample_repository()));
        let first_ticket = match open_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };
        let newer_commands = app.update(Action::Refresh);
        let newer_ticket = match newer_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };
        app.on_status_refreshed(newer_ticket, Ok(clean_status()));

        app.on_status_refreshed(
            first_ticket,
            Err(GitSailError::new(ErrorCode::ProcessFailure, "boom")),
        );

        assert_eq!(
            app.view_phase(),
            ViewPhase::Loaded,
            "a stale error must not downgrade an already-loaded view"
        );
        assert!(app.status_error().is_none());
    }

    #[test]
    fn navigation_is_not_blocked_by_a_slow_status_refresh() {
        let (gate_tx, gate_rx) = mpsc::channel();
        let port = Arc::new(FakePort {
            repository: sample_repository(),
            status: Mutex::new(clean_status()),
            branches: vec![sample_branch("main", true)],
            status_gate: Some(Mutex::new(gate_rx)),
        });
        let (mut app, commands) = App::new(PathBuf::from("/repo"), port.clone() as _, false);
        assert!(matches!(commands.as_slice(), [Command::OpenRepository(_)]));

        let open_commands = app.on_repository_opened(Ok(sample_repository()));
        let (ticket, repo) = match open_commands.as_slice() {
            [Command::RefreshStatus(t, r), Command::LoadBranches(_, _)] => (*t, r.clone()),
            other => panic!("unexpected commands: {other:?}"),
        };

        let (tx, rx) = mpsc::channel();
        let port_dyn: Arc<dyn RepositoryReadPort> = port.clone();
        std::thread::spawn(move || {
            let result = gitsail_application::GetRepositoryStatus::new(port_dyn).execute(&repo);
            let _ = tx.send(result);
        });

        // While that call is blocked on the gate, the UI thread must still
        // be free to handle input (US-041 criteria 1-2).
        let focus_before = app.focus();
        app.update(Action::FocusNext);
        assert_ne!(
            app.focus(),
            focus_before,
            "focus must change while a refresh is outstanding"
        );
        assert_eq!(
            app.view_phase(),
            ViewPhase::Loading,
            "the status must still be pending — the gate has not been released yet"
        );

        gate_tx.send(()).unwrap();
        let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        app.on_status_refreshed(ticket, result);
        assert_eq!(app.view_phase(), ViewPhase::Loaded);
    }

    #[test]
    fn search_filters_the_branch_list_and_resets_the_cursor() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![
                sample_branch("main", true),
                sample_branch("develop", false),
            ]),
        );

        app.update(Action::MoveDown);
        assert_eq!(app.sidebar_cursor(), 1);

        app.update(Action::StartSearch);
        for c in "dev".chars() {
            app.update(Action::SearchInput(c));
        }

        assert_eq!(
            app.sidebar_cursor(),
            0,
            "starting a search resets the cursor"
        );
        assert_eq!(app.filtered_branches().len(), 1);
        assert_eq!(app.filtered_branches()[0].name.as_str(), "develop");
    }

    #[test]
    fn dismiss_closes_help_before_search_before_cancelling_an_operation() {
        let (mut app, _port) = new_app();
        app.update(Action::ToggleHelp);
        app.update(Action::StartSearch);
        assert!(app.help_visible());
        assert!(app.search().is_some());

        app.update(Action::Dismiss);
        assert!(
            !app.help_visible(),
            "the topmost overlay (help) closes first"
        );
        assert!(
            app.search().is_some(),
            "search is untouched while help was still open"
        );

        app.update(Action::Dismiss);
        assert!(app.search().is_none(), "the next dismiss closes search");
    }

    #[test]
    fn activate_on_the_sidebar_selects_the_highlighted_branch() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![
                sample_branch("main", true),
                sample_branch("develop", false),
            ]),
        );

        app.update(Action::MoveDown);
        app.update(Action::Activate);

        assert_eq!(
            app.session()
                .unwrap()
                .selection()
                .branch
                .as_ref()
                .map(BranchName::as_str),
            Some("develop")
        );
    }

    #[test]
    fn a_stale_branches_result_is_discarded() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let stale_generation = app.session().unwrap().generation();

        // A newer refresh bumps the generation before the branches result
        // for the older one arrives.
        app.update(Action::Refresh);

        app.on_branches_loaded(
            stale_generation,
            Ok(vec![sample_branch("stale-only", false)]),
        );

        assert!(
            app.filtered_branches().is_empty(),
            "a branches result computed for an old generation must not populate the sidebar"
        );
    }
}
