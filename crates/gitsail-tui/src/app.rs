//! Application state and the "Update" stage of SAD §18's event/update/
//! render model (US-040, US-041, US-046, US-047, US-048).
//!
//! [`App`] holds a [`RepositorySession`] (SAD §21) — the first real caller
//! of that type outside its own unit tests; the CLI (EPIC-08) uses the
//! read use cases directly and stays stateless between invocations, so a
//! session had no consumer until now. [`App::update`] is a pure function
//! from `(&mut App, Action)` to the [`Command`]s it wants run in the
//! background: it never touches a [`RepositoryReadPort`]/
//! [`RepositoryWritePort`] or spawns a thread itself, which is what makes
//! "operação lenta não trava navegação" (US-041) and "descarte de resultado
//! antigo" testable without a terminal or real timing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    export_patch, BlameRequest, CommitQuery, DiffRequest, Page, RefreshReason, RepositoryReadPort,
    RepositorySession,
};
use gitsail_domain::{
    Blame, Branch, BranchName, Commit, CommitGraph, CommitHash, Diff, GitSailError, GraphCommit,
    HeadState, Repository, RepositoryStatus,
};

use crate::action::Action;
use crate::clipboard::{ClipboardPort, SystemClipboard};
use crate::commit_search::parse_commit_search;
use crate::keymap::InputContext;
use crate::operation::{OperationKind, OperationState};
use crate::status_view::{build_status_entries, DiffScope, StatusEntry};
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

/// Which content the Diff panel is currently showing for the selected file
/// (US-046 criterion 3). Kept as a sub-mode of [`Panel::Diff`] rather than a
/// fifth [`Panel`] variant — both views apply to the same selected file and
/// share the same grid slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffViewMode {
    Diff,
    Blame,
}

/// Outcome of the last [`Action::ExportPatch`] (US-029 criterion 1: "origem
/// e escopo do patch são informados"). Shown as a transient banner in the
/// Diff panel until the next diff selection replaces it — never persisted
/// across a new file selection, exactly like `diff`/`diff_error` are reset
/// in [`App::load_selected_diff`], so a stale result from a previously
/// viewed file can never be mistaken for feedback about the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchExportOutcome {
    /// The patch was copied to the system clipboard (the primary action,
    /// criterion 3).
    Copied {
        scope: String,
        file_count: usize,
        incomplete: bool,
    },
    /// The clipboard was unavailable or failed, so the patch was saved to
    /// `path` instead (criterion 3's documented fallback). `reason` carries
    /// the clipboard failure so it is never silently swallowed.
    SavedToFile {
        scope: String,
        path: PathBuf,
        incomplete: bool,
        reason: String,
    },
    /// Neither the clipboard nor a fallback file could receive the patch.
    Failed { reason: String },
    /// There was nothing to export — every file in the current diff was
    /// binary, truncated, or had no content hunks. Distinct from `Failed`:
    /// nothing went wrong, there was simply no patchable content.
    Empty,
}

/// How many commits [`App`] requests per commit-graph page (US-065, US-066
/// criterion 3). Not tuned for any particular repository size — a large
/// enough value that a typical scroll session rarely needs a second
/// round-trip, small enough that opening a huge repository never blocks on
/// loading its entire history up front.
const GRAPH_PAGE_SIZE: u32 = 200;

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

    // -- US-066: commit graph panel --------------------------------------
    /// The shared, presentation-independent layout (US-064, US-065):
    /// [`gitsail_domain::graph`]'s module documentation is this field's
    /// lane-stability contract.
    commit_graph: CommitGraph,
    /// Full [`Commit`] metadata (subject, author, ...) parallel to
    /// `commit_graph.rows()` — [`CommitGraph::append_page`] always emits
    /// exactly one row per input commit, in the same order, so appending
    /// the same page's items here keeps both indexed identically.
    graph_commits: Vec<Commit>,
    graph_cursor: usize,
    graph_next_cursor: Option<String>,
    graph_has_more: bool,
    graph_loading: bool,
    graph_error: Option<GitSailError>,
    graph_request_id: u64,
    /// The commit-search box's text while it is being edited (US-045
    /// criterion 2), distinct from `search` (the Sidebar's branch-name
    /// filter, US-042) since the two apply to different panels and one
    /// submits a new paginated query while the other is a pure client-side
    /// filter.
    commit_search: Option<String>,
    /// The raw text of the last *submitted* commit search, or `None` when
    /// the graph shows the unfiltered log — kept only for display (the
    /// Graph panel's title), since the actual filters already live in the
    /// [`CommitQuery`] a submission dispatches.
    active_commit_filter: Option<String>,
    /// Whether the commit-details overlay (Enter on the Graph panel,
    /// US-045 criterion 3) is open. Reads the already-loaded
    /// [`Self::graph_commits`] entry under `graph_cursor` rather than
    /// issuing a fresh [`gitsail_application::GetCommit`] call — the same
    /// `git log` invocation that filled the graph already returns every
    /// field (`%H%h%P%an%ae%cn%ce%ad%cd%s%b%D`) `GetCommit` itself would,
    /// so a second round-trip would only add latency for no new data.
    commit_details_open: bool,

    // -- US-046: status/diff/blame inspection --------------------------
    status_cursor: usize,
    selected_file: Option<StatusEntry>,
    diff: Option<Diff>,
    diff_error: Option<GitSailError>,
    diff_hunk_cursor: usize,
    diff_view_mode: DiffViewMode,
    diff_request_id: u64,
    blame: Option<Blame>,
    blame_error: Option<GitSailError>,
    blame_scroll: u16,
    blame_request_id: u64,

    // -- US-029: copy or export a patch -----------------------------------
    /// Presentation-only side effect, injected so tests can exercise both
    /// the copy-succeeds and clipboard-unavailable-falls-back-to-file paths
    /// (criterion 3) without touching a real OS clipboard. See
    /// [`crate::clipboard`]'s module docs for why this lives outside
    /// `gitsail-application`.
    clipboard: Arc<dyn ClipboardPort>,
    patch_export: Option<PatchExportOutcome>,

    // -- US-048: branch administration ----------------------------------
    branch_input: Option<String>,

    // -- US-047: stage/unstage/commit -----------------------------------
    commit_message: Option<String>,
    pending_paths: Vec<PathBuf>,

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
    ///
    /// Takes only a [`RepositoryReadPort`]: mutations (US-047, US-048) are
    /// dispatched as [`Command`]s that carry a cloned [`Repository`], never
    /// the write port itself — that port lives only in
    /// [`crate::worker::dispatch`], the single place any Git process (read
    /// or write) actually runs (SAD §18, §26). `port` is stored here only
    /// because [`RepositorySession::new`] needs it for its own synchronous
    /// `refresh` method.
    pub fn new(
        repo_path: PathBuf,
        port: Arc<dyn RepositoryReadPort>,
        low_color: bool,
    ) -> (App, Vec<Command>) {
        Self::new_with_clipboard(repo_path, port, low_color, Arc::new(SystemClipboard))
    }

    /// Like [`Self::new`], but takes the [`ClipboardPort`] explicitly
    /// rather than always constructing the real [`SystemClipboard`] — the
    /// entry point tests use to inject a fake clipboard (US-029 criterion
    /// 3). Production code (`main.rs`) always goes through [`Self::new`].
    pub fn new_with_clipboard(
        repo_path: PathBuf,
        port: Arc<dyn RepositoryReadPort>,
        low_color: bool,
        clipboard: Arc<dyn ClipboardPort>,
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
            commit_graph: CommitGraph::new(),
            graph_commits: Vec::new(),
            graph_cursor: 0,
            graph_next_cursor: None,
            graph_has_more: false,
            graph_loading: false,
            graph_error: None,
            graph_request_id: 0,
            commit_search: None,
            active_commit_filter: None,
            commit_details_open: false,
            status_cursor: 0,
            selected_file: None,
            diff: None,
            diff_error: None,
            diff_hunk_cursor: 0,
            diff_view_mode: DiffViewMode::Diff,
            diff_request_id: 0,
            blame: None,
            blame_error: None,
            blame_scroll: 0,
            blame_request_id: 0,
            clipboard,
            patch_export: None,
            branch_input: None,
            commit_message: None,
            pending_paths: Vec::new(),
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

    /// The branches loaded for the sidebar, independent of any mutation
    /// capability — reused by [`Self::request_checkout`]/
    /// [`Self::request_delete_branch`] to look up the highlighted branch.
    pub fn branches(&self) -> &[Branch] {
        &self.branches
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

    /// The status entries the Details panel lists (US-046 criterion 1),
    /// recomputed from the session's last snapshot on every call — cheap
    /// enough for a per-frame render, and it keeps a single source of truth
    /// instead of a second copy that could drift from `session.status()`.
    pub fn status_entries(&self) -> Vec<StatusEntry> {
        self.session
            .as_ref()
            .and_then(|s| s.status())
            .map(build_status_entries)
            .unwrap_or_default()
    }

    pub fn status_cursor(&self) -> usize {
        self.status_cursor
    }

    pub fn selected_file(&self) -> Option<&StatusEntry> {
        self.selected_file.as_ref()
    }

    pub fn diff_view_mode(&self) -> DiffViewMode {
        self.diff_view_mode
    }

    pub fn diff(&self) -> Option<&Diff> {
        self.diff.as_ref()
    }

    pub fn diff_error(&self) -> Option<&GitSailError> {
        self.diff_error.as_ref()
    }

    pub fn diff_hunk_cursor(&self) -> usize {
        self.diff_hunk_cursor
    }

    pub fn blame(&self) -> Option<&Blame> {
        self.blame.as_ref()
    }

    pub fn blame_error(&self) -> Option<&GitSailError> {
        self.blame_error.as_ref()
    }

    pub fn blame_scroll(&self) -> u16 {
        self.blame_scroll
    }

    /// The outcome of the last patch export/copy action (US-029 criterion
    /// 1), or `None` before one has ever run or after it was superseded by
    /// a new diff selection.
    pub fn patch_export(&self) -> Option<&PatchExportOutcome> {
        self.patch_export.as_ref()
    }

    pub fn branch_input(&self) -> Option<&str> {
        self.branch_input.as_deref()
    }

    /// The commit-graph layout loaded so far (US-064, US-065, US-066).
    pub fn commit_graph(&self) -> &CommitGraph {
        &self.commit_graph
    }

    /// Full commit metadata parallel to `commit_graph().rows()` (see the
    /// field's own doc comment for the index-alignment guarantee).
    pub fn graph_commits(&self) -> &[Commit] {
        &self.graph_commits
    }

    pub fn graph_cursor(&self) -> usize {
        self.graph_cursor
    }

    pub fn graph_has_more(&self) -> bool {
        self.graph_has_more
    }

    pub fn graph_loading(&self) -> bool {
        self.graph_loading
    }

    pub fn graph_error(&self) -> Option<&GitSailError> {
        self.graph_error.as_ref()
    }

    /// The commit-search box's text while it is being edited (US-045
    /// criterion 2), or `None` when the box is closed.
    pub fn commit_search(&self) -> Option<&str> {
        self.commit_search.as_deref()
    }

    /// The raw text of the last submitted commit search, or `None` while
    /// the graph shows the unfiltered log (US-045 criterion 2).
    pub fn active_commit_filter(&self) -> Option<&str> {
        self.active_commit_filter.as_deref()
    }

    /// Whether the commit-details overlay is open (US-045 criterion 3).
    pub fn commit_details_open(&self) -> bool {
        self.commit_details_open
    }

    /// The full [`Commit`] the Graph panel's cursor currently points at —
    /// the same commit the details overlay shows, and the same one a
    /// separate "list" panel would need to keep in sync by hash (US-045
    /// criterion 1). Since the Graph panel *is* the list (see this crate's
    /// module docs on the Graph panel), staying in sync is simply reading
    /// this one cursor rather than reconciling two.
    pub fn selected_graph_commit(&self) -> Option<&Commit> {
        self.graph_commits.get(self.graph_cursor)
    }

    pub fn commit_message(&self) -> Option<&str> {
        self.commit_message.as_deref()
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
    /// search/prompts never let a hidden action through). Priority: the
    /// help overlay always wins; then the commit-details overlay (US-045
    /// criterion 3); then the commit composer, but only while no operation
    /// is confirming/running — the moment `Enter` moves it to `Confirming`,
    /// this falls through to `Normal` so the *second* `Enter` is handled by
    /// the confirmation intercept in [`Self::handle_activate`] instead of
    /// re-editing the message; then the branch-name prompt; then the
    /// commit-search box (US-045 criterion 2); then the branch-filter
    /// search. Every one of these is opened by its own distinct action, so
    /// at most one is ever `Some`/`true` at a time — the order here only
    /// documents which this function would prefer, not a real conflict.
    pub fn input_context(&self) -> InputContext {
        if self.help_visible {
            InputContext::Help
        } else if self.commit_details_open {
            InputContext::CommitDetails
        } else if self.commit_message.is_some() && self.operation.is_idle() {
            InputContext::CommitMessage
        } else if self.branch_input.is_some() {
            InputContext::BranchName
        } else if self.commit_search.is_some() {
            InputContext::CommitSearch
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
    /// blocks and never touches `self.port`/`self.write_port` beyond
    /// cloning the `Arc`/repository into a `Command` for the caller to
    /// dispatch (US-041 criterion 2).
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
            Action::MoveUp => self.move_cursor(-1),
            Action::MoveDown => self.move_cursor(1),
            Action::Activate => self.handle_activate(),
            Action::Dismiss => {
                self.dismiss();
                Vec::new()
            }
            Action::ToggleHelp => {
                self.help_visible = !self.help_visible;
                Vec::new()
            }
            // Gated by focus exactly like `Action::StartCreateBranch`
            // below: the same `/` key means "filter the branch list" on
            // the Sidebar (US-042) and "search commits" on the Graph panel
            // (US-045 criterion 2) — two different panels' own concern,
            // never a shared text box.
            Action::StartSearch => {
                if self.focus == Panel::Graph {
                    self.commit_search =
                        Some(self.active_commit_filter.clone().unwrap_or_default());
                } else {
                    self.search = Some(String::new());
                    self.sidebar_cursor = 0;
                }
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
            Action::Refresh => self.refresh_commands_for(RefreshReason::Manual),
            Action::Quit => {
                self.should_quit = true;
                Vec::new()
            }
            Action::ToggleBlameView => self.toggle_blame_view(),
            Action::StartCreateBranch => {
                if self.focus == Panel::Sidebar {
                    self.branch_input = Some(String::new());
                }
                Vec::new()
            }
            Action::BranchNameInput(c) => {
                if let Some(text) = self.branch_input.as_mut() {
                    text.push(c);
                }
                Vec::new()
            }
            Action::BranchNameBackspace => {
                if let Some(text) = self.branch_input.as_mut() {
                    text.pop();
                }
                Vec::new()
            }
            Action::RequestCheckout => {
                self.request_checkout();
                Vec::new()
            }
            Action::RequestDeleteBranch => {
                self.request_delete_branch();
                Vec::new()
            }
            Action::ToggleStage => self.request_toggle_stage(),
            Action::StartCommit => {
                if self.commit_message.is_none() {
                    self.commit_message = Some(String::new());
                }
                Vec::new()
            }
            Action::CommitMessageInput(c) => {
                if let Some(text) = self.commit_message.as_mut() {
                    text.push(c);
                }
                Vec::new()
            }
            Action::CommitMessageBackspace => {
                if let Some(text) = self.commit_message.as_mut() {
                    text.pop();
                }
                Vec::new()
            }
            Action::CommitSearchInput(c) => {
                if let Some(query) = self.commit_search.as_mut() {
                    query.push(c);
                }
                Vec::new()
            }
            Action::CommitSearchBackspace => {
                if let Some(query) = self.commit_search.as_mut() {
                    query.pop();
                }
                Vec::new()
            }
            Action::CommitSearchSubmit => self.submit_commit_search(),
            Action::ExportPatch => {
                self.export_patch();
                Vec::new()
            }
        }
    }

    fn cyclic_cursor(current: usize, delta: i32, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let next = (current as i32 + delta).rem_euclid(len as i32);
        next as usize
    }

    fn move_cursor(&mut self, delta: i32) -> Vec<Command> {
        match self.focus {
            Panel::Sidebar => {
                self.sidebar_cursor =
                    Self::cyclic_cursor(self.sidebar_cursor, delta, self.filtered_branches().len());
                Vec::new()
            }
            Panel::Details => {
                self.status_cursor =
                    Self::cyclic_cursor(self.status_cursor, delta, self.status_entries().len());
                Vec::new()
            }
            Panel::Diff => {
                match self.diff_view_mode {
                    DiffViewMode::Diff => {
                        let hunks = self
                            .diff
                            .as_ref()
                            .and_then(|d| d.files.first())
                            .map(|f| f.hunks.len())
                            .unwrap_or(0);
                        self.diff_hunk_cursor =
                            Self::cyclic_cursor(self.diff_hunk_cursor, delta, hunks);
                    }
                    DiffViewMode::Blame => {
                        let max_scroll = self
                            .blame
                            .as_ref()
                            .map(|b| b.lines.len())
                            .unwrap_or(0)
                            .saturating_sub(1) as i32;
                        let next = (self.blame_scroll as i32 + delta).clamp(0, max_scroll.max(0));
                        self.blame_scroll = next as u16;
                    }
                }
                Vec::new()
            }
            Panel::Graph => self.move_graph_cursor(delta),
        }
    }

    /// Moves the Graph panel's selection, clamped (not cyclic — reaching
    /// the last loaded row is meaningful) to the loaded rows. Scrolling
    /// past the last loaded row while more history is available triggers
    /// loading the next page (US-066 criterion 3: "scroll... funciona
    /// corretamente com paginação"), without ever firing a second request
    /// while one is already in flight (`graph_loading`).
    fn move_graph_cursor(&mut self, delta: i32) -> Vec<Command> {
        let len = self.commit_graph.rows().len();
        if len == 0 {
            return Vec::new();
        }
        let next = (self.graph_cursor as i32 + delta).clamp(0, len as i32 - 1) as usize;
        let reached_end = delta > 0 && next + 1 >= len;
        self.graph_cursor = next;
        if reached_end {
            self.request_more_graph_commits()
        } else {
            Vec::new()
        }
    }

    /// Requests the next commit-graph page, continuing the same
    /// [`CommitGraph`] (US-065) rather than starting over. A no-op while a
    /// request is already outstanding or the last page already reported no
    /// more history (US-041-style discard-by-flag, mirroring
    /// `graph_loading` against `graph_has_more` instead of a request id,
    /// since there is nothing to discard until a result actually arrives).
    fn request_more_graph_commits(&mut self) -> Vec<Command> {
        if self.graph_loading || !self.graph_has_more {
            return Vec::new();
        }
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        self.graph_loading = true;
        self.graph_request_id += 1;
        let query = CommitQuery {
            limit: Some(GRAPH_PAGE_SIZE),
            cursor: self.graph_next_cursor.clone(),
            ..CommitQuery::default()
        };
        vec![Command::LoadCommitGraph(self.graph_request_id, repo, query)]
    }

    /// Submits the commit-search box's current text (US-045 criterion 2),
    /// closing it and parsing it into [`CommitQuery`] filters via
    /// [`parse_commit_search`] — never a bespoke TUI-only filter — before
    /// restarting the commit graph under those filters. An all-whitespace
    /// submission clears [`Self::active_commit_filter`] and returns to the
    /// unfiltered log, exactly like dismissing search does for the
    /// Sidebar's branch filter.
    fn submit_commit_search(&mut self) -> Vec<Command> {
        let text = self.commit_search.take().unwrap_or_default();
        let trimmed = text.trim();
        self.active_commit_filter = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        self.restart_commit_graph(parse_commit_search(trimmed))
    }

    /// Resets the commit graph to a brand new, empty page and requests its
    /// first page under `filters` (US-045 criterion 2), exactly like
    /// [`Self::on_repository_opened`] seeds the very first, unfiltered
    /// load. A search is a new query, not an addition to whatever was
    /// already loaded, so rows from a previous filter (or the unfiltered
    /// log) must never linger mixed in with the new result — bumping
    /// `graph_request_id` also makes any in-flight page for the *previous*
    /// query discarded by [`Self::on_commit_graph_page_loaded`] once it
    /// arrives late, the same staleness policy every other background read
    /// in this module already uses.
    fn restart_commit_graph(&mut self, filters: CommitQuery) -> Vec<Command> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        self.commit_graph = CommitGraph::new();
        self.graph_commits.clear();
        self.graph_cursor = 0;
        self.graph_next_cursor = None;
        self.graph_has_more = false;
        self.graph_error = None;
        self.graph_loading = true;
        self.graph_request_id += 1;
        let query = CommitQuery {
            limit: Some(GRAPH_PAGE_SIZE),
            ..filters
        };
        vec![Command::LoadCommitGraph(self.graph_request_id, repo, query)]
    }

    /// Intercepts `Enter` for the modes that give it a meaning beyond
    /// per-panel activation, then falls through to [`Self::activate`]:
    /// starting a branch name means confirming it, confirming a pending
    /// operation means dispatching it, otherwise the active panel decides.
    fn handle_activate(&mut self) -> Vec<Command> {
        if self.input_context() == InputContext::BranchName {
            let name = self.branch_input.take().unwrap_or_default();
            self.operation.begin(OperationKind::CreateBranch { name });
            return Vec::new();
        }
        if self.input_context() == InputContext::CommitMessage {
            self.operation.begin(OperationKind::CreateCommit);
            return Vec::new();
        }
        if let OperationState::Confirming(kind) = &self.operation {
            let kind = kind.clone();
            self.operation.confirm();
            return self.dispatch_operation(kind);
        }
        self.activate()
    }

    fn activate(&mut self) -> Vec<Command> {
        match self.focus {
            Panel::Sidebar => {
                let Some(branch) = self
                    .filtered_branches()
                    .get(self.sidebar_cursor)
                    .map(|b| b.name.clone())
                else {
                    return Vec::new();
                };
                if let Some(session) = self.session.as_mut() {
                    session.select_branch(branch);
                }
                Vec::new()
            }
            Panel::Details => self.load_selected_diff(),
            Panel::Graph => {
                self.open_commit_details();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Opens the commit-details overlay for the commit currently under the
    /// Graph panel's cursor (US-045 criterion 3), or does nothing when no
    /// commit is loaded yet (e.g. an empty repository). See
    /// [`Self::selected_graph_commit`] and the `commit_details_open` field
    /// doc for why this never dispatches a [`Command`]: the data is
    /// already resident from the graph's own load.
    fn open_commit_details(&mut self) {
        if self.selected_graph_commit().is_some() {
            self.commit_details_open = true;
        }
    }

    /// Loads the diff for the status entry under the cursor (US-046
    /// criterion 1: "abre o diff correto"), resetting any previously loaded
    /// diff/blame so a stale view is never shown for the new selection.
    fn load_selected_diff(&mut self) -> Vec<Command> {
        let entries = self.status_entries();
        let Some(entry) = entries.get(self.status_cursor).cloned() else {
            return Vec::new();
        };
        self.selected_file = Some(entry.clone());
        self.diff = None;
        self.diff_error = None;
        self.diff_hunk_cursor = 0;
        self.diff_view_mode = DiffViewMode::Diff;
        self.blame = None;
        self.blame_error = None;
        self.blame_scroll = 0;
        self.diff_request_id += 1;
        // A patch-export result names the file/scope it was generated for
        // (US-029 criterion 1); it must never linger once that scope is no
        // longer what is shown, or it would silently describe the wrong
        // diff.
        self.patch_export = None;

        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        let request = DiffRequest {
            staged: matches!(entry.scope, DiffScope::Staged),
            path_filter: Some(entry.path.clone()),
            ..DiffRequest::default()
        };
        vec![Command::LoadDiff(self.diff_request_id, repo, request)]
    }

    /// Toggles the Diff panel between diff and blame content for the
    /// currently selected file (US-046 criterion 3). Always reloads blame
    /// on switching to it rather than tracking whether a cached result
    /// still matches the selection — one `git blame` call is cheap, and
    /// this avoids a second staleness policy alongside the request-id one
    /// already used for both diff and blame results.
    fn toggle_blame_view(&mut self) -> Vec<Command> {
        self.diff_view_mode = match self.diff_view_mode {
            DiffViewMode::Diff => DiffViewMode::Blame,
            DiffViewMode::Blame => DiffViewMode::Diff,
        };
        if self.diff_view_mode != DiffViewMode::Blame {
            return Vec::new();
        }
        let Some(entry) = self.selected_file.clone() else {
            return Vec::new();
        };
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        self.blame_request_id += 1;
        let repo = session.repository().clone();
        let content_version = session.generation();
        let request = BlameRequest {
            file: entry.path.clone(),
            revision: None,
            line_range: None,
            buffer_contents: None,
        };
        vec![Command::LoadBlame(
            self.blame_request_id,
            repo,
            request,
            content_version,
        )]
    }

    /// Copies the currently displayed diff's patch to the system clipboard,
    /// falling back to saving it as a file when the clipboard is
    /// unavailable or fails (US-029/T-162).
    ///
    /// A no-op outside the Diff panel — `y` has no meaning elsewhere, the
    /// same gating [`Self::request_toggle_stage`] applies to `s` on the
    /// Details panel. Reads only [`Self::diff`], the data already loaded
    /// for on-screen display; this never issues a fresh [`Command`] and
    /// never touches the repository (criterion 2) — `export_patch` is a
    /// pure transform over data already in memory.
    fn export_patch(&mut self) {
        if self.focus != Panel::Diff {
            return;
        }
        let Some(diff) = self.diff.as_ref() else {
            self.patch_export = Some(PatchExportOutcome::Empty);
            return;
        };
        let export = export_patch(&diff.files);
        if export.is_empty() {
            self.patch_export = Some(PatchExportOutcome::Empty);
            return;
        }

        let scope = self.patch_scope_label();
        let file_count = export.included_files.len();
        let incomplete = export.is_incomplete();
        self.patch_export = Some(match self.clipboard.set_text(&export.patch) {
            Ok(()) => PatchExportOutcome::Copied {
                scope,
                file_count,
                incomplete,
            },
            Err(clipboard_reason) => match save_patch_to_file(&export.patch) {
                Ok(path) => PatchExportOutcome::SavedToFile {
                    scope,
                    path,
                    incomplete,
                    reason: clipboard_reason,
                },
                Err(save_reason) => PatchExportOutcome::Failed {
                    reason: format!(
                        "clipboard unavailable ({clipboard_reason}); saving to a file also failed ({save_reason})"
                    ),
                },
            },
        });
    }

    /// A short, human-readable description of what the exported patch
    /// covers (US-029 criterion 1: "origem e escopo do patch são
    /// informados") — which side of [`DiffScope`] and which file, mirroring
    /// [`OperationKind::target_label`]'s "always name the target" rule for
    /// mutations.
    fn patch_scope_label(&self) -> String {
        match &self.selected_file {
            Some(entry) => {
                let scope = match entry.scope {
                    DiffScope::Staged => "staged",
                    DiffScope::Worktree => "unstaged",
                };
                format!("{scope} changes — {}", entry.path.display())
            }
            None => "the current diff".to_string(),
        }
    }

    fn dismiss(&mut self) {
        if self.help_visible {
            self.help_visible = false;
        } else if self.commit_details_open {
            self.commit_details_open = false;
        } else if self.commit_message.is_some() && self.operation.is_idle() {
            // A confirmation in flight (`Confirming(CreateCommit)`) is left
            // alone here — cancelling *that* is `operation.cancel()` below,
            // which never touches `commit_message`, so a cancelled
            // confirmation returns to an editable composer with the typed
            // message intact.
            self.commit_message = None;
        } else if self.branch_input.is_some() {
            self.branch_input = None;
        } else if self.commit_search.is_some() {
            // Closes the box without touching `active_commit_filter`/the
            // loaded graph — an unsubmitted edit is discarded exactly like
            // `branch_input` above, never applied as a side effect of
            // merely leaving the box.
            self.commit_search = None;
        } else if self.search.is_some() {
            self.search = None;
            self.sidebar_cursor = 0;
        } else if self.patch_export.is_some() {
            self.patch_export = None;
        } else {
            self.operation.cancel();
        }
    }

    /// Stages or unstages the status entry under the cursor (US-047
    /// criterion 1: "seleção de arquivos invoca casos de uso
    /// compartilhados"). Both are `OperationRisk::Safe` (SAD §20), so this
    /// skips the explicit `Confirming` step other mutations go through —
    /// only `Moderate`/`Destructive` operations ask for a second `Enter`.
    fn request_toggle_stage(&mut self) -> Vec<Command> {
        if self.focus != Panel::Details {
            return Vec::new();
        }
        let entries = self.status_entries();
        let Some(entry) = entries.get(self.status_cursor) else {
            return Vec::new();
        };
        let kind = match entry.scope {
            DiffScope::Worktree => OperationKind::StageFiles,
            DiffScope::Staged => OperationKind::UnstageFiles,
        };
        self.pending_paths = vec![entry.path.clone()];
        self.operation.begin(kind.clone());
        self.operation.confirm();
        self.dispatch_operation(kind)
    }

    /// Starts confirmation for checking out the highlighted branch
    /// (US-048). A no-op on the current branch — there is nothing to
    /// switch to.
    fn request_checkout(&mut self) {
        if self.focus != Panel::Sidebar {
            return;
        }
        let Some(branch) = self
            .filtered_branches()
            .get(self.sidebar_cursor)
            .map(|b| (*b).clone())
        else {
            return;
        };
        if branch.is_current {
            return;
        }
        self.operation.begin(OperationKind::SwitchBranch {
            target: branch.name.as_str().to_string(),
        });
    }

    /// Starts confirmation for deleting the highlighted branch (US-048).
    /// Never forces the delete — Git's own refusal of an unmerged branch
    /// (US-023) is what makes forcing a separate, `Destructive` decision a
    /// future story can surface explicitly; this always requests the safe
    /// path first.
    fn request_delete_branch(&mut self) {
        if self.focus != Panel::Sidebar {
            return;
        }
        let Some(branch) = self
            .filtered_branches()
            .get(self.sidebar_cursor)
            .map(|b| (*b).clone())
        else {
            return;
        };
        if branch.is_current {
            return;
        }
        self.operation.begin(OperationKind::DeleteBranch {
            name: branch.name.as_str().to_string(),
            force: false,
        });
    }

    /// Turns a confirmed [`OperationKind`] into the [`Command`] that
    /// actually runs it. A malformed name (only possible from a
    /// hand-typed branch name — `Branch`/`StatusEntry`-derived kinds are
    /// always well-formed) fails the operation immediately rather than ever
    /// reaching the write port (criterion 3: nothing is discarded, and
    /// nothing is attempted with data known to be invalid).
    fn dispatch_operation(&mut self, kind: OperationKind) -> Vec<Command> {
        let Some(session) = self.session.as_ref() else {
            self.operation.cancel();
            return Vec::new();
        };
        let repo = session.repository().clone();
        match kind {
            OperationKind::StageFiles => {
                let paths = std::mem::take(&mut self.pending_paths);
                vec![Command::StageFiles(repo, paths)]
            }
            OperationKind::UnstageFiles => {
                let paths = std::mem::take(&mut self.pending_paths);
                vec![Command::UnstageFiles(repo, paths)]
            }
            OperationKind::CreateCommit => {
                let message = self.commit_message.clone().unwrap_or_default();
                vec![Command::CreateCommit(repo, message)]
            }
            OperationKind::SwitchBranch { target } => match BranchName::new(target) {
                Ok(name) => vec![Command::SwitchBranch(repo, name)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::CreateBranch { name } => match BranchName::new(name) {
                Ok(name) => vec![Command::CreateBranch(repo, name, None)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::DeleteBranch { name, force } => match BranchName::new(name) {
                Ok(name) => vec![Command::DeleteBranch(repo, name, force)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
        }
    }

    fn refresh_commands_for(&mut self, reason: RefreshReason) -> Vec<Command> {
        let Some(session) = self.session.as_mut() else {
            return Vec::new();
        };
        let ticket = session.begin_refresh(reason);
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

                // A freshly opened (or re-opened) repository starts a brand
                // new commit graph rather than appending to whatever the
                // previous repository's session had loaded, and drops any
                // search/details state that referred to that old graph.
                self.active_commit_filter = None;
                self.commit_search = None;
                self.commit_details_open = false;

                let mut commands = vec![
                    Command::RefreshStatus(ticket, repo.clone()),
                    Command::LoadBranches(generation, repo),
                ];
                commands.extend(self.restart_commit_graph(CommitQuery::default()));
                commands
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
    ///
    /// A successful refresh also clears any selected file/diff/blame: the
    /// status it was computed against may no longer be current (a file may
    /// have been staged, reverted, or changed by another process), and
    /// re-selecting from the fresh list is cheaper and safer than trying to
    /// carry an old selection forward.
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
                self.status_cursor = 0;
                self.selected_file = None;
                self.diff = None;
                self.diff_error = None;
                self.diff_hunk_cursor = 0;
                self.diff_view_mode = DiffViewMode::Diff;
                self.blame = None;
                self.blame_error = None;
                self.blame_scroll = 0;
            }
            Err(error) => {
                self.status_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::CommitGraphPageLoaded`] (US-065,
    /// US-066), discarding a result computed for a since-abandoned request
    /// (e.g. the repository was reopened before this page arrived) exactly
    /// like [`Self::on_diff_loaded`]. A kept result is folded into
    /// `commit_graph` via [`CommitGraph::append_page`] — never replacing
    /// it — so earlier pages' rows, lanes, and edges are preserved
    /// unchanged (US-065 criterion 3).
    pub fn on_commit_graph_page_loaded(
        &mut self,
        request_id: u64,
        result: Result<Page<Commit>, GitSailError>,
    ) {
        if request_id != self.graph_request_id {
            return;
        }
        self.graph_loading = false;
        match result {
            Ok(page) => {
                let graph_commits: Vec<GraphCommit> =
                    page.items.iter().map(GraphCommit::from).collect();
                self.commit_graph.append_page(&graph_commits);
                self.graph_commits.extend(page.items);
                self.graph_next_cursor = page.next_cursor;
                self.graph_has_more = page.has_more;
                self.graph_error = None;
            }
            Err(error) => {
                self.graph_error = Some(error);
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

    /// Handles [`crate::message::Message::DiffLoaded`] (US-046), discarding
    /// a result computed for a since-abandoned selection.
    pub fn on_diff_loaded(&mut self, request_id: u64, result: Result<Diff, GitSailError>) {
        if request_id != self.diff_request_id {
            return;
        }
        match result {
            Ok(diff) => {
                self.diff = Some(diff);
                self.diff_error = None;
            }
            Err(error) => {
                self.diff_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::BlameLoaded`] (US-046), matching
    /// [`Self::on_diff_loaded`]'s staleness handling.
    pub fn on_blame_loaded(&mut self, request_id: u64, result: Result<Blame, GitSailError>) {
        if request_id != self.blame_request_id {
            return;
        }
        match result {
            Ok(blame) => {
                self.blame = Some(blame);
                self.blame_error = None;
            }
            Err(error) => {
                self.blame_error = Some(error);
            }
        }
    }

    /// Handles the completion of a `StageFiles`/`UnstageFiles`/
    /// `SwitchBranch`/`CreateBranch`/`DeleteBranch` [`Command`] (US-047,
    /// US-048). Success moves the operation to `Succeeded` and refreshes
    /// status/branches (criterion 3: "sucesso provoca refresh"/"sucesso
    /// atualiza status/histórico"); failure moves it to `Failed` and
    /// changes nothing else — no refresh, no state discarded (US-048
    /// criterion 3: "mudanças incompatíveis mostram erro sem descarte").
    ///
    /// A `Safe`-risk success (stage/unstage) never showed a confirmation in
    /// the first place ([`Self::request_toggle_stage`]), so it returns
    /// straight to `Idle` instead of lingering as a `Succeeded` state that
    /// would need dismissing — otherwise it would still be sitting there,
    /// blocking [`Self::input_context`] from recognizing the commit
    /// composer, the next time the person opens one.
    pub fn on_operation_finished(&mut self, result: Result<(), GitSailError>) -> Vec<Command> {
        match result {
            Ok(()) => {
                self.operation.succeed();
                if matches!(&self.operation, OperationState::Succeeded(kind) if kind.risk() == crate::operation::OperationRisk::Safe)
                {
                    self.operation.cancel();
                }
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::CommitCreated`] (US-047). Success
    /// clears the composer and refreshes; failure preserves the typed
    /// message and the staged index exactly as they were (criterion 3).
    pub fn on_commit_created(&mut self, result: Result<CommitHash, GitSailError>) -> Vec<Command> {
        match result {
            Ok(_hash) => {
                self.operation.succeed();
                self.commit_message = None;
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }
}

/// Saves `patch` to a fresh, uniquely named file in the current working
/// directory (US-029 criterion 3's documented fallback when the clipboard
/// is unavailable). A nanosecond timestamp keeps repeated exports from
/// colliding without needing a counter shared across calls.
fn save_patch_to_file(patch: &str) -> Result<PathBuf, String> {
    let dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = dir.join(format!("gitsail-patch-{nanos}.patch"));
    std::fs::write(&path, patch).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::FakeClipboard;
    use gitsail_domain::{
        BranchKind, ChangeType, DiffHunk, DiffLine, DiffLineOrigin, ErrorCode, FileChange,
        FileDiff, FileStatusCode, RepositoryId,
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

    fn new_app_with_clipboard(clipboard: Arc<dyn ClipboardPort>) -> (App, Arc<FakePort>) {
        let port = port_without_gate();
        let (app, commands) =
            App::new_with_clipboard(PathBuf::from("/repo"), port.clone() as _, false, clipboard);
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
            [Command::RefreshStatus(t, r), Command::LoadBranches(_, _), Command::LoadCommitGraph(_, _, _)] => {
                (*t, r.clone())
            }
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
            [Command::RefreshStatus(t, r), Command::LoadBranches(_, _), Command::LoadCommitGraph(_, _, _)] => {
                (*t, r.clone())
            }
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

    fn open_and_load_dirty_status(app: &mut App) {
        let open_commands = app.on_repository_opened(Ok(sample_repository()));
        let ticket = match open_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };
        app.on_status_refreshed(ticket, Ok(dirty_status()));
    }

    #[test]
    fn selecting_a_status_entry_requests_its_diff_and_a_stale_result_is_discarded() {
        let (mut app, _port) = new_app();
        open_and_load_dirty_status(&mut app);
        app.update(Action::FocusNext); // Sidebar -> Graph
        app.update(Action::FocusNext); // Graph -> Details

        let commands = app.update(Action::Activate);
        let request_id = match commands.as_slice() {
            [Command::LoadDiff(id, _, _)] => *id,
            other => panic!("expected exactly one LoadDiff command, got {other:?}"),
        };
        assert_eq!(app.selected_file().unwrap().path, PathBuf::from("a.txt"));

        // A second selection bumps the request id before the first result
        // arrives — the stale one must not populate the diff.
        let newer_commands = app.update(Action::Activate);
        let newer_id = match newer_commands.as_slice() {
            [Command::LoadDiff(id, _, _)] => *id,
            other => panic!("expected exactly one LoadDiff command, got {other:?}"),
        };
        assert_ne!(request_id, newer_id);

        app.on_diff_loaded(
            request_id,
            Ok(Diff {
                files: vec![FileDiff {
                    path: PathBuf::from("a.txt"),
                    previous_path: None,
                    change_type: ChangeType::Modified,
                    is_binary: false,
                    truncated: false,
                    hunks: vec![],
                }],
            }),
        );
        assert!(
            app.diff().is_none(),
            "a stale diff result must be discarded"
        );

        app.on_diff_loaded(
            newer_id,
            Ok(Diff {
                files: vec![FileDiff {
                    path: PathBuf::from("a.txt"),
                    previous_path: None,
                    change_type: ChangeType::Modified,
                    is_binary: false,
                    truncated: false,
                    hunks: vec![],
                }],
            }),
        );
        assert!(app.diff().is_some());
    }

    #[test]
    fn toggling_stage_on_a_worktree_entry_dispatches_stage_files_without_confirmation() {
        let (mut app, _port) = new_app();
        open_and_load_dirty_status(&mut app);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext);

        let commands = app.update(Action::ToggleStage);
        match commands.as_slice() {
            [Command::StageFiles(_, paths)] => assert_eq!(paths, &[PathBuf::from("a.txt")]),
            other => panic!("expected exactly one StageFiles command, got {other:?}"),
        }
        assert!(
            matches!(
                app.operation(),
                OperationState::InProgress(OperationKind::StageFiles)
            ),
            "a Safe operation must skip the Confirming step"
        );
    }

    #[test]
    fn checking_out_the_current_branch_is_a_no_op() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![sample_branch("main", true)]),
        );

        app.update(Action::RequestCheckout);
        assert!(app.operation().is_idle());
    }

    #[test]
    fn checking_out_another_branch_confirms_then_dispatches_switch_branch() {
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

        app.update(Action::RequestCheckout);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::SwitchBranch { .. })
        ));

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::SwitchBranch(_, name)] => assert_eq!(name.as_str(), "develop"),
            other => panic!("expected exactly one SwitchBranch command, got {other:?}"),
        }
    }

    #[test]
    fn cancelling_a_pending_branch_confirmation_never_dispatches_anything() {
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
        app.update(Action::RequestDeleteBranch);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::DeleteBranch { .. })
        ));

        app.update(Action::Dismiss);
        assert!(app.operation().is_idle());
    }

    #[test]
    fn creating_a_branch_types_a_name_then_confirms_then_dispatches() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(app.session().unwrap().generation(), Ok(vec![]));

        app.update(Action::StartCreateBranch);
        for c in "feature/x".chars() {
            app.update(Action::BranchNameInput(c));
        }
        app.update(Action::Activate);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::CreateBranch { .. })
        ));
        assert!(
            app.branch_input().is_none(),
            "the prompt closes once the name is committed to the confirmation"
        );

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::CreateBranch(_, name, None)] => assert_eq!(name.as_str(), "feature/x"),
            other => panic!("expected exactly one CreateBranch command, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_operation_never_refreshes_and_leaves_state_untouched() {
        let (mut app, _port) = new_app();
        open_and_load_dirty_status(&mut app);
        app.update(Action::FocusNext);
        app.update(Action::FocusNext);
        app.update(Action::ToggleStage);

        let commands =
            app.on_operation_finished(Err(GitSailError::new(ErrorCode::OperationConflict, "boom")));
        assert!(
            commands.is_empty(),
            "a failure must never trigger a refresh"
        );
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
    }

    #[test]
    fn composing_a_commit_shows_the_message_and_a_failure_preserves_it() {
        let (mut app, _port) = new_app();
        open_and_load_dirty_status(&mut app);

        app.update(Action::StartCommit);
        for c in "fix bug".chars() {
            app.update(Action::CommitMessageInput(c));
        }
        assert_eq!(app.commit_message(), Some("fix bug"));

        app.update(Action::Activate);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::CreateCommit)
        ));

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::CreateCommit(_, message)] => assert_eq!(message, "fix bug"),
            other => panic!("expected exactly one CreateCommit command, got {other:?}"),
        }

        let refresh_commands = app.on_commit_created(Err(GitSailError::new(
            ErrorCode::ProcessFailure,
            "hook rejected",
        )));
        assert!(refresh_commands.is_empty(), "a failure must never refresh");
        assert_eq!(
            app.commit_message(),
            Some("fix bug"),
            "a failed commit must preserve the typed message"
        );
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
    }

    #[test]
    fn a_successful_commit_clears_the_composer_and_refreshes() {
        let (mut app, _port) = new_app();
        open_and_load_dirty_status(&mut app);

        app.update(Action::StartCommit);
        app.update(Action::CommitMessageInput('x'));
        app.update(Action::Activate);
        app.update(Action::Activate);

        let commands = app.on_commit_created(Ok(CommitHash::new(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        )
        .unwrap()));
        assert!(
            matches!(
                commands.as_slice(),
                [Command::RefreshStatus(_, _), Command::LoadBranches(_, _)]
            ),
            "success must refresh status and branches"
        );
        assert!(app.commit_message().is_none());
        assert!(matches!(app.operation(), OperationState::Succeeded(_)));
    }

    // -- US-029: copy or export a patch -----------------------------------

    fn modified_a_txt_diff() -> Diff {
        Diff {
            files: vec![FileDiff {
                path: PathBuf::from("a.txt"),
                previous_path: None,
                change_type: ChangeType::Modified,
                is_binary: false,
                truncated: false,
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![
                        DiffLine {
                            origin: DiffLineOrigin::Deletion,
                            content: "old".to_string(),
                            has_trailing_newline: true,
                        },
                        DiffLine {
                            origin: DiffLineOrigin::Addition,
                            content: "new".to_string(),
                            has_trailing_newline: true,
                        },
                    ],
                }],
            }],
        }
    }

    /// Drives `app` from a fresh, opened session to the Diff panel focused
    /// with `diff` loaded for the single worktree entry `a.txt` — the setup
    /// every `export_patch` test below needs.
    fn app_with_diff_focused(app: &mut App, diff: Diff) {
        open_and_load_dirty_status(app);
        app.update(Action::FocusNext); // Sidebar -> Graph
        app.update(Action::FocusNext); // Graph -> Details
        let commands = app.update(Action::Activate); // load the diff for a.txt
        let request_id = match commands.as_slice() {
            [Command::LoadDiff(id, _, _)] => *id,
            other => panic!("expected exactly one LoadDiff command, got {other:?}"),
        };
        app.on_diff_loaded(request_id, Ok(diff));
        app.update(Action::FocusNext); // Details -> Diff
        assert_eq!(app.focus(), Panel::Diff);
    }

    #[test]
    fn export_patch_copies_the_current_diff_and_reports_its_scope() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        app.update(Action::ExportPatch);

        let copied = clipboard.last_set.lock().unwrap().clone();
        assert_eq!(
            copied.as_deref(),
            Some("--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n"),
            "the clipboard must receive exactly the rendered git-apply-compatible patch"
        );
        match app.patch_export() {
            Some(PatchExportOutcome::Copied {
                scope,
                file_count,
                incomplete,
            }) => {
                assert!(
                    scope.contains("a.txt") && scope.contains("unstaged"),
                    "the scope must name the origin (unstaged) and the file, got {scope:?}"
                );
                assert_eq!(*file_count, 1);
                assert!(!incomplete);
            }
            other => panic!("expected Copied, got {other:?}"),
        }
    }

    #[test]
    fn export_patch_falls_back_to_a_file_when_the_clipboard_is_unavailable() {
        let clipboard = Arc::new(FakeClipboard {
            fail: true,
            ..Default::default()
        });
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        app.update(Action::ExportPatch);

        assert!(
            clipboard.last_set.lock().unwrap().is_none(),
            "a failed clipboard write must never be recorded as successful"
        );
        let path = match app.patch_export() {
            Some(PatchExportOutcome::SavedToFile {
                scope,
                path,
                incomplete,
                reason,
            }) => {
                assert!(scope.contains("a.txt"));
                assert!(!incomplete);
                assert!(
                    !reason.is_empty(),
                    "the clipboard failure reason must be carried, never swallowed"
                );
                path.clone()
            }
            other => panic!("expected SavedToFile, got {other:?}"),
        };

        let saved = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("expected the fallback file to exist at {path:?}: {e}"));
        assert_eq!(saved, "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n");
        std::fs::remove_file(&path).expect("clean up the fallback file created by this test");
    }

    #[test]
    fn export_patch_outside_the_diff_panel_is_a_no_op() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        open_and_load_dirty_status(&mut app);
        app.update(Action::FocusNext); // Sidebar -> Graph
        app.update(Action::FocusNext); // Graph -> Details
        assert_eq!(app.focus(), Panel::Details);

        app.update(Action::ExportPatch);

        assert!(app.patch_export().is_none());
        assert!(clipboard.last_set.lock().unwrap().is_none());
    }

    #[test]
    fn export_patch_with_nothing_loaded_reports_empty_rather_than_copying_stale_content() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        open_and_load_dirty_status(&mut app);
        app.update(Action::FocusNext); // Sidebar -> Graph
        app.update(Action::FocusNext); // Graph -> Details
        app.update(Action::FocusNext); // Details -> Diff
        assert_eq!(app.focus(), Panel::Diff);

        app.update(Action::ExportPatch);

        assert_eq!(app.patch_export(), Some(&PatchExportOutcome::Empty));
        assert!(clipboard.last_set.lock().unwrap().is_none());
    }

    #[test]
    fn export_patch_of_a_binary_only_diff_reports_empty_without_fabricating_a_patch() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        app_with_diff_focused(
            &mut app,
            Diff {
                files: vec![FileDiff {
                    path: PathBuf::from("a.txt"),
                    previous_path: None,
                    change_type: ChangeType::Modified,
                    is_binary: true,
                    truncated: false,
                    hunks: vec![],
                }],
            },
        );

        app.update(Action::ExportPatch);

        assert_eq!(app.patch_export(), Some(&PatchExportOutcome::Empty));
        assert!(clipboard.last_set.lock().unwrap().is_none());
    }

    #[test]
    fn selecting_a_new_diff_clears_a_previous_patch_export_result() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard.clone());
        app_with_diff_focused(&mut app, modified_a_txt_diff());
        app.update(Action::ExportPatch);
        assert!(app.patch_export().is_some());

        // Re-selecting the same entry from Details re-triggers
        // `load_selected_diff`, which must invalidate the now-stale
        // export result rather than let it silently describe a diff that
        // is no longer the one on screen.
        app.update(Action::FocusPrev); // Diff -> Details
        app.update(Action::Activate);

        assert!(
            app.patch_export().is_none(),
            "a new diff selection must clear the previous export result"
        );
    }
}
