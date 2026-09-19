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
    export_patch, AmendPreview, ApplyPatchResult, BlameRequest, CherryPickResult, CommitQuery,
    DiffRequest, GetForgeLink, MergeParentPolicy, MergeResult, Page, PatchPreview, PullOutcome,
    RebaseAction, RebasePlan, RebaseResult, RefreshReason, RepositoryReadPort, RepositorySession,
    ResetMode, RevertResult,
};
use gitsail_domain::{
    Blame, Branch, BranchKind, BranchName, Commit, CommitGraph, CommitHash, ConflictSide,
    ConflictSides, Diff, ErrorCode, ForgePath, GitSailError, GraphCommit, HeadState,
    InProgressOperation, OperationCapability, ReflogEntry, Remote, Repository, RepositoryStatus,
    Stash, Tag,
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
    /// Lists tags, remotes and stash entries (US-050), switching between
    /// them via [`ReferenceView`] rather than three separate `Panel`
    /// variants — see that type's own doc.
    References,
}

impl Panel {
    const ALL: [Panel; 5] = [
        Panel::Sidebar,
        Panel::Graph,
        Panel::Details,
        Panel::Diff,
        Panel::References,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Sidebar => "Sidebar",
            Panel::Graph => "Graph",
            Panel::Details => "Details",
            Panel::Diff => "Diff",
            Panel::References => "References",
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

/// Which list the References panel currently shows (US-050 criterion 1).
/// Kept as a sub-mode of [`Panel::References`], cycled by a dedicated key
/// (`t`), rather than three separate `Panel` variants — mirroring
/// [`DiffViewMode`]'s own rationale: all three apply to the same panel slot
/// and share one selection cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceView {
    Tags,
    Remotes,
    Stash,
    /// `HEAD`'s reflog (T-241/US-089) — folded into this panel's existing
    /// cycle of sub-views rather than a new [`Panel`] variant: the fixed
    /// three-column bottom layout (`crate::ui::render`) has no spare column,
    /// and a reflog entry list is exactly the same shape (an indexed,
    /// selectable list of hash/date/message records) every other sub-view
    /// here already is.
    Reflog,
}

impl ReferenceView {
    pub fn title(self) -> &'static str {
        match self {
            ReferenceView::Tags => "Tags",
            ReferenceView::Remotes => "Remotes",
            ReferenceView::Stash => "Stash",
            ReferenceView::Reflog => "Reflog",
        }
    }

    /// Cycles to the next sub-view in a fixed, documented order (mirrors
    /// [`Panel::next`]).
    fn next(self) -> Self {
        match self {
            ReferenceView::Tags => ReferenceView::Remotes,
            ReferenceView::Remotes => ReferenceView::Stash,
            ReferenceView::Stash => ReferenceView::Reflog,
            ReferenceView::Reflog => ReferenceView::Tags,
        }
    }
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

/// Outcome of the last [`Action::RequestApplyPatch`] (T-163/US-030),
/// mirroring [`PatchExportOutcome`]'s "always state the outcome
/// explicitly, never leave it to be inferred" convention. Shown as a
/// transient banner in the Diff panel, replaced by the next apply attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchApplyOutcome {
    /// The clipboard had nothing usable to apply — empty, or unavailable
    /// (`reason` carries why). Nothing was previewed or confirmed.
    ClipboardEmpty { reason: String },
    /// The preview (`git apply --check`) rejected the patch — malformed,
    /// referencing a path outside the repository, or with a context that
    /// no longer matches the current file content (US-030 criterion 2).
    /// Confirmation is never reached in this case.
    Rejected { reason: String },
    /// The confirmed apply itself failed — e.g. the file changed again
    /// between preview and confirmation. Reported honestly, never as an
    /// implicit rollback (US-030 criterion 3).
    Failed { reason: String },
    /// The patch was applied. Carries exactly the files touched (US-030
    /// criterion 3: never a generic "done").
    Applied { affected_files: Vec<PathBuf> },
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

    // -- US-050: tags/remotes/stash inspection -----------------------------
    tags: Vec<Tag>,
    remotes: Vec<Remote>,
    stashes: Vec<Stash>,
    reference_view: ReferenceView,
    reference_cursor: usize,
    reference_details_open: bool,

    // -- US-049: sync with a remote -----------------------------------------
    /// A remote/branch resolution failure (US-049 criterion 1: never guess
    /// across several equally-plausible remotes) — a side-channel banner
    /// like [`Self::patch_export`], not modeled through [`OperationState`]
    /// since nothing was ever confirmed or dispatched.
    sync_error: Option<GitSailError>,
    /// The outcome of the last successful [`Action::RequestPull`] (US-049
    /// criterion 1: divergence/"nothing to integrate" must be shown
    /// explicitly, not collapsed into a bare success) — mirrors
    /// [`Self::patch_export`]'s "transient banner, cleared by the next
    /// relevant action" convention.
    last_pull_outcome: Option<PullOutcome>,
    /// The operation that last succeeded, kept only so the status bar can
    /// report its outcome *after* the overlay closes itself (T-267).
    ///
    /// A success needs no acknowledgement — the confirmation already
    /// happened, before anything mutated — so making the person dismiss a
    /// "Done" popup was pure friction, and the popup could not even be
    /// dismissed (GitHub issue #1). Only a failure still holds the screen,
    /// because US-044 criterion 3 requires the error and its remediation to
    /// be readable. The per-outcome detail the overlay used to show
    /// (fast-forward vs. merge commit vs. conflict, US-049/079/083/086/087)
    /// is rendered from the `last_*_result` fields beside this one, so
    /// closing the overlay never collapses those distinctions into a bare
    /// "Done". Cleared by the next dispatched operation, and by `Esc` once
    /// nothing else is open.
    last_operation_outcome: Option<OperationKind>,

    // -- T-243/US-101: open a forge link in the browser ---------------------
    /// A failure to actually launch the browser (T-243/US-101) — a
    /// transient banner like [`Self::sync_error`], never a reason to
    /// touch anything else in `App`'s state.
    forge_link_error: Option<GitSailError>,

    // -- US-029: copy or export a patch -----------------------------------
    /// Presentation-only side effect, injected so tests can exercise both
    /// the copy-succeeds and clipboard-unavailable-falls-back-to-file paths
    /// (criterion 3) without touching a real OS clipboard. See
    /// [`crate::clipboard`]'s module docs for why this lives outside
    /// `gitsail-application`.
    clipboard: Arc<dyn ClipboardPort>,
    patch_export: Option<PatchExportOutcome>,

    // -- T-163/US-030: apply a patch --------------------------------------
    /// Transient banner mirroring [`Self::patch_export`]'s own convention.
    patch_apply_outcome: Option<PatchApplyOutcome>,
    /// The exact clipboard text a supported preview was just built from,
    /// held onto so the confirmed [`crate::worker::Command::ApplyPatch`]
    /// reuses it unchanged rather than re-reading a clipboard that may
    /// have changed since (criterion 1's preview and criterion 3's applied
    /// result must always refer to the same patch). Cleared once the
    /// pending operation is dispatched or cancelled.
    pending_patch_text: Option<String>,

    // -- US-048: branch administration ----------------------------------
    branch_input: Option<String>,

    // -- T-157/US-024: rename branch --------------------------------------
    /// The branch's previous name, set while its rename prompt is open —
    /// `branch_input` (above) doubles as the editable new-name field for
    /// both create and rename, and this is what tells the two apart
    /// (`Some` means "renaming", `None` means "creating"). Pre-filled into
    /// `branch_input` when the prompt opens ([`Self::request_rename_branch`])
    /// so the field always shows both the previous and new name at once
    /// (criterion 1).
    rename_source: Option<String>,
    /// Set right before dispatching a confirmed [`OperationKind::RenameBranch`]
    /// (both names already validated), consumed the next time
    /// [`Self::on_branches_loaded`] applies a fresh list — that is the one
    /// place both `session.selection()` and `sidebar_cursor` can be
    /// re-pointed at the branch's new name without losing track of it
    /// (criterion 3: a renamed selected/current branch must never be
    /// "lost"). Cleared unconditionally at the top of every
    /// [`Self::dispatch_operation`] call and on a failed rename (no refresh
    /// follows a failure, so nothing would ever consume it otherwise).
    pending_branch_rename: Option<(BranchName, BranchName)>,

    // -- US-047: stage/unstage/commit -----------------------------------
    commit_message: Option<String>,
    pending_paths: Vec<PathBuf>,

    help_visible: bool,
    search: Option<String>,
    operation: OperationState,
    should_quit: bool,
    low_color: bool,
    frame_size: (u16, u16),

    // -- EPIC-16/T-231..T-233: merge, conflicts, continue/abort -----------
    /// The freshest known merge/rebase/cherry-pick/revert/bisect state
    /// (T-230/US-078), loaded after every refresh and after a merge/
    /// continue/abort mutation — never assumed from GitSail's own last
    /// action, so an operation started in another terminal, or the real
    /// outcome of a just-run continue/abort (US-081 criterion 3: "resultado
    /// real é reinspecionado"), is always what is actually shown.
    in_progress_operation: InProgressOperation,
    /// Whether the conflicts overlay (`M`) is open. A no-op to open when
    /// [`Self::in_progress_operation`] has no conflicted files (T-232/
    /// US-080 criterion 1).
    conflicts_open: bool,
    /// Which conflicted file (an index into
    /// `in_progress_operation.conflicted_files()`) is highlighted in the
    /// overlay.
    conflict_cursor: usize,
    /// The base/ours/theirs sides last loaded for inspection (T-232/US-080
    /// criterion 2), cleared whenever the highlighted file changes or a
    /// resolution action runs, so a stale inspection can never be mistaken
    /// for the newly selected file's content.
    inspected_conflict: Option<ConflictSides>,
    /// A failure from loading conflict sides or resolving a conflict,
    /// shown inline in the overlay — side-channel like
    /// [`Self::patch_export`], not modeled through [`OperationState`] since
    /// mark-resolved/take-side dispatch immediately (mirrors
    /// [`Self::request_toggle_stage`]'s `Safe`-risk "skip confirmation"
    /// convention) rather than going through a confirm step.
    conflict_error: Option<GitSailError>,
    /// The outcome of the last successful [`Action::RequestMerge`] (T-231/
    /// US-079 criterion 2: fast-forward, merge commit and conflict are
    /// always three distinct, explicit outcomes) — mirrors
    /// [`Self::last_pull_outcome`]'s own "transient banner" convention.
    last_merge_result: Option<MergeResult>,
    /// The outcome of the last successful [`Action::RequestRebase`] (T-235/
    /// US-083 criterion 3: completion and conflict are always two distinct,
    /// explicit outcomes) — mirrors [`Self::last_merge_result`]'s own
    /// "transient banner" convention. Also the outcome of a successful
    /// [`Action::RequestRebasePlan`] confirmation (T-236/US-084): both
    /// [`crate::worker::Command::Rebase`] and
    /// [`crate::worker::Command::ExecuteRebasePlan`] report through the same
    /// [`crate::message::Message::RebaseFinished`]/[`Self::on_rebase_finished`]
    /// path, since both ultimately produce the same [`RebaseResult`].
    last_rebase_result: Option<RebaseResult>,

    // -- T-236/US-084: plan an interactive rebase -------------------------
    /// Whether the interactive rebase plan overlay (`O`, Sidebar only) is
    /// open. Kept `true` through `Confirming`/`InProgress` once a plan is
    /// submitted (mirrors [`Self::commit_message`]'s own "stays around
    /// through confirmation" convention) so [`Self::dispatch_operation`] can
    /// still read the exact plan being executed; only actually cleared the
    /// moment that dispatch happens, or the overlay is dismissed outright.
    rebase_plan_open: bool,
    /// The interactive rebase plan currently being edited: the exact
    /// candidate commit range [`gitsail_application::PlanRebase`] returned,
    /// oldest first, each entry's action/message reassignable in place
    /// before [`Self::dispatch_operation`] hands the whole thing to
    /// [`crate::worker::Command::ExecuteRebasePlan`] (T-236/US-084 criterion
    /// 1). `None` while the overlay is closed, or while a freshly requested
    /// plan is still loading — [`Self::rebase_plan_open`] tracks the overlay
    /// itself separately, so a loading/error state can still be shown while
    /// this stays `None`.
    rebase_plan: Option<RebasePlan>,
    /// Which entry (an index into `rebase_plan`'s `entries`) is highlighted.
    rebase_plan_cursor: usize,
    /// The Reword message prompt's buffer, `Some` while editing the
    /// highlighted entry's replacement message — mirrors
    /// [`Self::commit_message`], reused per this task's own instruction to
    /// reuse the existing text-input mechanism rather than build a new one.
    rebase_plan_reword_input: Option<String>,
    /// A plan-load failure, or a client-side [`RebasePlan::validate`]
    /// failure surfaced before ever reaching
    /// [`crate::worker::Command::ExecuteRebasePlan`] (T-236/US-084 criterion
    /// 2's "plano inválido não executa") — shown inline in the overlay like
    /// [`Self::conflict_error`]. Never the final authority: the real,
    /// authoritative revalidation always happens in
    /// `RepositoryWritePort::execute_rebase_plan` itself; this is only an
    /// immediate, client-side echo of the same rules for faster feedback,
    /// and a stale-plan refusal from the Core still surfaces through the
    /// ordinary `OperationState::Failed` path, never through this field.
    rebase_plan_error: Option<GitSailError>,

    // -- T-238/T-239/US-086/US-087: cherry-pick, revert -------------------
    /// The outcome of the last successful [`Action::RequestCherryPick`]
    /// (T-238/US-086 criterion 3: applying, a conflict, and an empty
    /// "already applied" result are always three distinct, explicit
    /// outcomes) — mirrors [`Self::last_merge_result`]'s own "transient
    /// banner" convention.
    last_cherry_pick_result: Option<CherryPickResult>,
    /// The outcome of the last successful [`Action::RequestRevert`] (T-239/
    /// US-087 criterion 2), mirroring [`Self::last_cherry_pick_result`].
    last_revert_result: Option<RevertResult>,

    // -- T-240/US-088: reset -----------------------------------------------
    /// Whether the reset-mode chooser overlay (`z`, Graph panel only) is
    /// open — lets a person pick soft/mixed/hard before anything is
    /// confirmed (US-088 criterion 1: each mode's distinct effect is shown
    /// up front). Mirrors [`Self::rebase_plan_open`]'s own "stays open
    /// through confirmation" convention.
    reset_mode_open: bool,
    /// Which [`ResetMode`] is highlighted in the chooser, as an index into
    /// `[Soft, Mixed, Hard]` (in that fixed order — least to most
    /// destructive).
    reset_mode_cursor: usize,
    /// The commit the chooser was opened against (the Graph panel's
    /// highlighted commit at the moment `z` was pressed) — captured once so
    /// the target stays stable while the chooser is open, even if the Graph
    /// cursor itself moves under an unrelated key.
    reset_target: Option<Commit>,

    // -- T-241/US-089: inspect HEAD's reflog -------------------------------
    /// `HEAD`'s reflog entries, newest first (US-089 criterion 1) — shown
    /// as [`ReferenceView::Reflog`], one of [`Self::reference_view`]'s
    /// existing sub-views, sharing its own `reference_cursor`.
    reflog: Vec<ReflogEntry>,
    /// Whether the reflog-entry commit-details overlay is open (US-089
    /// criterion 2), mirroring [`Self::reference_details_open`].
    reflog_details_open: bool,
    /// The full [`Commit`] loaded for the highlighted reflog entry, via the
    /// same [`gitsail_application::GetCommit`] use case
    /// [`Self::commit_details_open`]'s overlay already reuses — never a
    /// parallel read (US-089 criterion 2). `None` while loading, or when
    /// the entry's object no longer exists ([`Self::reflog_details_error`]
    /// carries that case instead, US-089 criterion 3).
    reflog_details_commit: Option<Commit>,
    /// A load failure, or the "this entry's object no longer exists"
    /// message set directly by [`Self::open_reflog_details`] without any
    /// read at all (US-089 criterion 3) — shown inline in the overlay,
    /// mirroring [`Self::conflict_error`]'s own side-channel convention.
    reflog_details_error: Option<GitSailError>,

    // -- T-242/US-090: amend the last commit -------------------------------
    /// Whether the amend composer is open (`A`). Kept `true` through
    /// `Confirming`/`InProgress`/`Failed` (mirrors [`Self::commit_message`]'s
    /// own "stays open through confirmation" convention, not
    /// [`Self::reset_mode_open`]'s "hands off to the generic operation
    /// overlay" one) — US-090 criterion 3 requires a failed amend to never
    /// lose the typed message, so the composer (and the message inside it)
    /// must still be showing afterward, not just still resident in memory.
    amend_open: bool,
    /// The read-only preview [`gitsail_application::PreviewAmend`] returned:
    /// `HEAD`'s exact commit (identity for the confirmation, US-090
    /// criterion 2) and the staged diff that would be folded in. `None`
    /// while loading, or after a failed preview
    /// ([`Self::amend_error`] carries that case).
    amend_preview: Option<AmendPreview>,
    /// The amend message being edited, pre-filled from
    /// [`Self::amend_preview`]'s `head` subject/body the moment it loads
    /// (mirrors `apps/desktop/src/stores/amend.ts`'s own `loadPreview`
    /// prefill exactly) and otherwise editable like [`Self::commit_message`].
    /// Never cleared on a failed amend (US-090 criterion 3) — only ever
    /// cleared by a successful amend or by dismissing the composer outright.
    amend_message: Option<String>,
    /// A preview-load failure, shown inline in the composer, mirroring
    /// [`Self::rebase_plan_error`]'s own convention.
    amend_error: Option<GitSailError>,
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
            tags: Vec::new(),
            remotes: Vec::new(),
            stashes: Vec::new(),
            reference_view: ReferenceView::Tags,
            reference_cursor: 0,
            reference_details_open: false,
            sync_error: None,
            last_pull_outcome: None,
            last_operation_outcome: None,
            forge_link_error: None,
            clipboard,
            patch_export: None,
            patch_apply_outcome: None,
            pending_patch_text: None,
            branch_input: None,
            rename_source: None,
            pending_branch_rename: None,
            commit_message: None,
            pending_paths: Vec::new(),
            help_visible: false,
            search: None,
            operation: OperationState::default(),
            should_quit: false,
            low_color,
            frame_size: (0, 0),
            in_progress_operation: InProgressOperation::None,
            conflicts_open: false,
            conflict_cursor: 0,
            inspected_conflict: None,
            conflict_error: None,
            last_merge_result: None,
            last_rebase_result: None,
            rebase_plan_open: false,
            rebase_plan: None,
            rebase_plan_cursor: 0,
            rebase_plan_reword_input: None,
            rebase_plan_error: None,
            last_cherry_pick_result: None,
            last_revert_result: None,
            reset_mode_open: false,
            reset_mode_cursor: 0,
            reset_target: None,
            reflog: Vec::new(),
            reflog_details_open: false,
            reflog_details_commit: None,
            reflog_details_error: None,
            amend_open: false,
            amend_preview: None,
            amend_message: None,
            amend_error: None,
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

    /// The freshest known in-progress-operation state (T-230/US-078),
    /// consumed by both the TUI's own conflicts overlay and (once wired) any
    /// mutation guard.
    pub fn in_progress_operation(&self) -> &InProgressOperation {
        &self.in_progress_operation
    }

    pub fn conflicts_open(&self) -> bool {
        self.conflicts_open
    }

    pub fn conflict_cursor(&self) -> usize {
        self.conflict_cursor
    }

    pub fn inspected_conflict(&self) -> Option<&ConflictSides> {
        self.inspected_conflict.as_ref()
    }

    pub fn conflict_error(&self) -> Option<&GitSailError> {
        self.conflict_error.as_ref()
    }

    pub fn last_merge_result(&self) -> Option<&MergeResult> {
        self.last_merge_result.as_ref()
    }

    pub fn last_rebase_result(&self) -> Option<&RebaseResult> {
        self.last_rebase_result.as_ref()
    }

    pub fn last_cherry_pick_result(&self) -> Option<&CherryPickResult> {
        self.last_cherry_pick_result.as_ref()
    }

    pub fn last_revert_result(&self) -> Option<&RevertResult> {
        self.last_revert_result.as_ref()
    }

    /// Whether the reset-mode chooser overlay (`z`, T-240/US-088) is open.
    pub fn reset_mode_open(&self) -> bool {
        self.reset_mode_open
    }

    /// The chooser's highlighted mode, as an index into `[Soft, Mixed,
    /// Hard]`.
    pub fn reset_mode_cursor(&self) -> usize {
        self.reset_mode_cursor
    }

    /// The commit the reset-mode chooser was opened against.
    pub fn reset_target(&self) -> Option<&Commit> {
        self.reset_target.as_ref()
    }

    /// The concrete count of uncommitted changes a `Hard` reset would
    /// permanently discard right now (US-088 criterion 2) — every currently
    /// staged or unstaged change, from the same already-loaded status this
    /// crate's status/diff panels already show, never a second read and
    /// never a generic "some changes" estimate. Shown live in the
    /// reset-mode chooser (before any mode is even picked) and carried
    /// verbatim into [`crate::operation::OperationKind::Reset`] the moment
    /// `Hard` is confirmed, so the confirmation prompt's number is always
    /// exactly what the chooser already showed.
    pub fn predicted_reset_loss_file_count(&self) -> usize {
        self.status_entries().len()
    }

    /// `HEAD`'s reflog entries loaded so far (T-241/US-089 criterion 1),
    /// newest first.
    pub fn reflog(&self) -> &[ReflogEntry] {
        &self.reflog
    }

    /// Whether the reflog-entry commit-details overlay is open (US-089
    /// criterion 2).
    pub fn reflog_details_open(&self) -> bool {
        self.reflog_details_open
    }

    /// The full commit loaded for the highlighted reflog entry, or `None`
    /// while loading (see [`Self::reflog_details_error`] for the other two
    /// "no commit to show" cases: a load failure, and the entry's object no
    /// longer existing at all).
    pub fn reflog_details_commit(&self) -> Option<&Commit> {
        self.reflog_details_commit.as_ref()
    }

    /// A reflog-details load failure, or the fixed "object no longer
    /// exists" message [`Self::open_reflog_details`] sets directly for an
    /// expired/pruned entry (US-089 criterion 3) — never a read is even
    /// attempted for that case.
    pub fn reflog_details_error(&self) -> Option<&GitSailError> {
        self.reflog_details_error.as_ref()
    }

    /// Whether the amend composer (`A`, T-242/US-090) is open.
    pub fn amend_open(&self) -> bool {
        self.amend_open
    }

    /// The read-only amend preview loaded so far (US-090 criterion 1), or
    /// `None` while loading or after a failed preview.
    pub fn amend_preview(&self) -> Option<&AmendPreview> {
        self.amend_preview.as_ref()
    }

    /// The amend message currently being edited (US-090 criterion 3: never
    /// cleared by a failed amend).
    pub fn amend_message(&self) -> Option<&str> {
        self.amend_message.as_deref()
    }

    /// A preview-load failure, shown inline in the composer.
    pub fn amend_error(&self) -> Option<&GitSailError> {
        self.amend_error.as_ref()
    }

    /// Whether the interactive rebase plan overlay (`O`, T-236/US-084) is
    /// open.
    pub fn rebase_plan_open(&self) -> bool {
        self.rebase_plan_open
    }

    /// The interactive rebase plan currently being edited, or `None` while
    /// the overlay is closed or a freshly requested plan is still loading.
    pub fn rebase_plan(&self) -> Option<&RebasePlan> {
        self.rebase_plan.as_ref()
    }

    pub fn rebase_plan_cursor(&self) -> usize {
        self.rebase_plan_cursor
    }

    /// The Reword message prompt's buffer, or `None` when it is not open.
    pub fn rebase_plan_reword_input(&self) -> Option<&str> {
        self.rebase_plan_reword_input.as_deref()
    }

    /// A plan-load or client-side validation failure (T-236/US-084
    /// criterion 2), shown inline in the overlay.
    pub fn rebase_plan_error(&self) -> Option<&GitSailError> {
        self.rebase_plan_error.as_ref()
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

    /// The local tags loaded so far (US-050 criterion 1).
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// The configured remotes loaded so far (US-050 criterion 1).
    pub fn remotes(&self) -> &[Remote] {
        &self.remotes
    }

    /// The stash entries loaded so far (US-050 criterion 1), newest first.
    pub fn stashes(&self) -> &[Stash] {
        &self.stashes
    }

    pub fn reference_view(&self) -> ReferenceView {
        self.reference_view
    }

    pub fn reference_cursor(&self) -> usize {
        self.reference_cursor
    }

    /// Whether the reference-details overlay is open (US-050 criterion 2).
    pub fn reference_details_open(&self) -> bool {
        self.reference_details_open
    }

    /// The number of entries in the currently active reference sub-view —
    /// what [`Self::move_cursor`]/[`Self::open_reference_details`] index
    /// into, and what `ui` uses to know whether the cursor highlight
    /// applies to a real row or the sub-view's own empty-state text.
    pub fn reference_len(&self) -> usize {
        match self.reference_view {
            ReferenceView::Tags => self.tags.len(),
            ReferenceView::Remotes => self.remotes.len(),
            ReferenceView::Stash => self.stashes.len(),
            ReferenceView::Reflog => self.reflog.len(),
        }
    }

    /// A remote/branch resolution failure from the last
    /// [`Action::RequestFetch`]/[`Action::RequestPull`]/
    /// [`Action::RequestPush`] (US-049 criterion 1), or `None` once
    /// dismissed or superseded by a successful resolution.
    pub fn sync_error(&self) -> Option<&GitSailError> {
        self.sync_error.as_ref()
    }

    /// The outcome of the last successful pull (US-049 criterion 1).
    pub fn last_pull_outcome(&self) -> Option<&PullOutcome> {
        self.last_pull_outcome.as_ref()
    }

    /// The operation that last succeeded, for the status bar's own outcome
    /// report (T-267) — see [`Self::last_operation_outcome`]'s field doc.
    pub fn last_operation_outcome(&self) -> Option<&OperationKind> {
        self.last_operation_outcome.as_ref()
    }

    /// A failure to launch the browser for the last
    /// [`Action::RequestOpenForgeLink`] (T-243/US-101), or `None` before
    /// one has ever run or after it was superseded.
    pub fn forge_link_error(&self) -> Option<&GitSailError> {
        self.forge_link_error.as_ref()
    }

    /// The destination [`Action::RequestOpenForgeLink`] would currently
    /// resolve to, from the focused panel's selection:
    /// - Sidebar (Branches view), highlighted branch -> that branch's page.
    /// - Graph panel, highlighted commit -> that commit's page.
    /// - Anything else -> the repository's own root page.
    ///
    /// `None` means no configured remote resolves to a known forge (US-101
    /// criterion 3) — this is the single source of truth for both whether
    /// a "open in browser" hint/action is offered at all and what it opens,
    /// so the two can never disagree.
    fn forge_path(&self) -> ForgePath {
        match self.focus {
            Panel::Sidebar => self
                .filtered_branches()
                .get(self.sidebar_cursor)
                .map(|b| ForgePath::Branch(b.name.clone())),
            Panel::Graph => self
                .selected_graph_commit()
                .map(|c| ForgePath::Commit(c.hash.clone())),
            _ => None,
        }
        .unwrap_or(ForgePath::Repository)
    }

    /// The actual browser URL [`Action::RequestOpenForgeLink`] would open,
    /// or `None` when no configured remote resolves to a known forge.
    pub fn forge_link_target(&self) -> Option<String> {
        GetForgeLink::execute(&self.remotes, self.forge_path())
    }

    /// Opens [`Self::forge_link_target`] in the browser (`w`). A no-op —
    /// never an error banner — when no remote resolves to a known forge
    /// (US-101 criterion 3): this is the normal case for most
    /// repositories, not a failure.
    fn request_open_forge_link(&mut self) -> Vec<Command> {
        self.forge_link_error = None;
        match self.forge_link_target() {
            Some(url) => vec![Command::OpenUrl(url)],
            None => Vec::new(),
        }
    }

    /// Handles [`crate::message::Message::UrlOpened`] (T-243/US-101):
    /// surfaces a launch failure as a transient banner, never as anything
    /// that blocks Git functionality.
    pub fn on_url_opened(&mut self, result: Result<(), GitSailError>) {
        self.forge_link_error = result.err();
    }

    /// The outcome of the last patch export/copy action (US-029 criterion
    /// 1), or `None` before one has ever run or after it was superseded by
    /// a new diff selection.
    pub fn patch_export(&self) -> Option<&PatchExportOutcome> {
        self.patch_export.as_ref()
    }

    /// The outcome of the last [`Action::RequestApplyPatch`] flow
    /// (T-163/US-030), mirroring [`Self::patch_export`].
    pub fn patch_apply_outcome(&self) -> Option<&PatchApplyOutcome> {
        self.patch_apply_outcome.as_ref()
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
    /// search/prompts never let a hidden action through). Priority: an
    /// operation that is confirming/running/terminal always wins, because
    /// its overlay is modal (T-267 — see below); then the
    /// help overlay; then the commit-details overlay (US-045
    /// criterion 3); then the reference-details overlay (US-050 criterion
    /// 2); then the commit composer — which, the moment `Enter` moves its
    /// operation to `Confirming`, is outranked by the modal check above, so
    /// the *second* `Enter` is handled by the confirmation intercept in
    /// [`Self::handle_activate`] instead of re-editing the message (the
    /// conflicts, rebase-plan, reset-mode and amend surfaces all work the
    /// same way); then the branch-name prompt; then the
    /// commit-search box (US-045 criterion 2); then the branch-filter
    /// search. Every one of these is opened by its own distinct action, so
    /// at most one is ever `Some`/`true` at a time — the order here only
    /// documents which this function would prefer, not a real conflict.
    ///
    /// The operation overlay owning the keyboard is what fixes GitHub issue
    /// #1 (T-267): every context below is reached only while the operation
    /// is `Idle`, so before this early return the overlay fell through to
    /// [`InputContext::Normal`] — which binds no `Esc` at all — and no key
    /// could ever reach [`Self::dismiss`]/[`OperationState::cancel`]. The
    /// per-overlay `operation.is_idle()` guards the contexts below used to
    /// carry are therefore gone: this one check replaces all of them, and
    /// the second `Enter` that confirms a pending operation still reaches
    /// [`Self::handle_activate`]'s `Confirming` intercept rather than the
    /// composer/plan/chooser underneath, exactly as before.
    pub fn input_context(&self) -> InputContext {
        match self.operation {
            OperationState::Confirming(_) => return InputContext::OperationConfirm,
            OperationState::InProgress(_) => return InputContext::OperationRunning,
            OperationState::Succeeded(_) | OperationState::Failed(_, _) => {
                return InputContext::OperationResult
            }
            OperationState::Idle => {}
        }
        if self.help_visible {
            InputContext::Help
        } else if self.commit_details_open {
            InputContext::CommitDetails
        } else if self.reference_details_open {
            InputContext::ReferenceDetails
        } else if self.reflog_details_open {
            InputContext::ReflogDetails
        } else if self.conflicts_open {
            InputContext::Conflicts
        } else if self.rebase_plan_open && self.rebase_plan_reword_input.is_some() {
            // The Reword prompt is itself layered over the plan overlay, so
            // it must win over `InputContext::RebasePlan` below whenever it
            // is open.
            InputContext::RebasePlanReword
        } else if self.rebase_plan_open {
            InputContext::RebasePlan
        } else if self.reset_mode_open {
            InputContext::ResetMode
        } else if self.commit_message.is_some() {
            InputContext::CommitMessage
        } else if self.amend_open {
            // `Self::amend_open` (not the message buffer itself) is what
            // tracks whether the composer is open, since the message starts
            // `None` while the preview is still loading.
            InputContext::Amend
        } else if self.branch_input.is_some() && self.rename_source.is_some() {
            InputContext::RenameBranch
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
                    self.rename_source = None;
                }
                Vec::new()
            }
            Action::StartRenameBranch => {
                self.request_rename_branch();
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
            Action::RequestFetch => self.request_fetch(),
            Action::RequestPull => self.request_pull(),
            Action::RequestPush => self.request_push(),
            Action::CycleReferenceView => {
                if self.focus == Panel::References {
                    self.reference_view = self.reference_view.next();
                    self.reference_cursor = 0;
                }
                Vec::new()
            }
            Action::RequestApplyPatch => self.request_apply_patch(),
            Action::RequestMerge => {
                self.request_merge();
                Vec::new()
            }
            Action::ToggleConflictsPanel => {
                self.toggle_conflicts_panel();
                Vec::new()
            }
            Action::InspectConflict => self.inspect_conflict(),
            Action::MarkConflictResolved => self.request_mark_conflict_resolved(),
            Action::TakeConflictSideOurs => self.request_take_conflict_side(ConflictSide::Ours),
            Action::TakeConflictSideTheirs => self.request_take_conflict_side(ConflictSide::Theirs),
            Action::RequestContinueOperation => {
                self.request_continue_operation();
                Vec::new()
            }
            Action::RequestAbortOperation => {
                self.request_abort_operation();
                Vec::new()
            }
            Action::RequestRebase => {
                self.request_rebase();
                Vec::new()
            }
            Action::RequestSkipOperation => {
                self.request_skip_operation();
                Vec::new()
            }
            Action::RequestRebasePlan => self.request_rebase_plan(),
            Action::RebasePlanMoveEntryUp => {
                self.rebase_plan_move_entry_up();
                Vec::new()
            }
            Action::RebasePlanMoveEntryDown => {
                self.rebase_plan_move_entry_down();
                Vec::new()
            }
            Action::RebasePlanCycleAction => {
                self.rebase_plan_cycle_action();
                Vec::new()
            }
            Action::RebasePlanRewordInput(c) => {
                if let Some(text) = self.rebase_plan_reword_input.as_mut() {
                    text.push(c);
                }
                Vec::new()
            }
            Action::RebasePlanRewordBackspace => {
                if let Some(text) = self.rebase_plan_reword_input.as_mut() {
                    text.pop();
                }
                Vec::new()
            }
            Action::RequestCherryPick => {
                self.request_cherry_pick();
                Vec::new()
            }
            Action::RequestRevert => {
                self.request_revert();
                Vec::new()
            }
            Action::RequestReset => {
                self.request_reset();
                Vec::new()
            }
            Action::StartAmend => self.request_start_amend(),
            Action::AmendMessageInput(c) => {
                if let Some(text) = self.amend_message.as_mut() {
                    text.push(c);
                }
                Vec::new()
            }
            Action::AmendMessageBackspace => {
                if let Some(text) = self.amend_message.as_mut() {
                    text.pop();
                }
                Vec::new()
            }
            Action::RequestOpenForgeLink => self.request_open_forge_link(),
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
        if self.conflicts_open {
            let len = self.in_progress_operation.conflicted_files().len();
            let next = Self::cyclic_cursor(self.conflict_cursor, delta, len);
            if next != self.conflict_cursor {
                // A different file is now highlighted — the previously
                // inspected sides no longer describe it.
                self.inspected_conflict = None;
                self.conflict_error = None;
            }
            self.conflict_cursor = next;
            return Vec::new();
        }
        if self.rebase_plan_open {
            if let Some(plan) = self.rebase_plan.as_ref() {
                self.rebase_plan_cursor =
                    Self::cyclic_cursor(self.rebase_plan_cursor, delta, plan.entries.len());
            }
            return Vec::new();
        }
        if self.reset_mode_open {
            self.reset_mode_cursor = Self::cyclic_cursor(self.reset_mode_cursor, delta, 3);
            return Vec::new();
        }
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
            Panel::References => {
                self.reference_cursor =
                    Self::cyclic_cursor(self.reference_cursor, delta, self.reference_len());
                Vec::new()
            }
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
        if self.input_context() == InputContext::RenameBranch {
            let new_name = self.branch_input.take().unwrap_or_default();
            let old_name = self.rename_source.take().unwrap_or_default();
            self.operation
                .begin(OperationKind::RenameBranch { old_name, new_name });
            return Vec::new();
        }
        if self.input_context() == InputContext::BranchName {
            let name = self.branch_input.take().unwrap_or_default();
            self.operation.begin(OperationKind::CreateBranch { name });
            return Vec::new();
        }
        if self.input_context() == InputContext::CommitMessage {
            self.operation.begin(OperationKind::CreateCommit);
            return Vec::new();
        }
        if self.input_context() == InputContext::RebasePlanReword {
            self.confirm_rebase_plan_reword();
            return Vec::new();
        }
        if self.input_context() == InputContext::RebasePlan {
            self.confirm_rebase_plan();
            return Vec::new();
        }
        if self.input_context() == InputContext::ResetMode {
            self.confirm_reset_mode();
            return Vec::new();
        }
        if self.input_context() == InputContext::Amend {
            return self.confirm_amend();
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
            Panel::References => match self.reference_view {
                // T-241/US-089 criterion 2: selecting a reflog entry opens
                // its commit details when the object still exists, and this
                // is genuinely a fresh read (a reflog entry's own fields
                // carry a hash/message/date, never a full `Commit`) — unlike
                // [`Self::open_commit_details`]/[`Self::open_reference_details`],
                // which only ever flip a bool over already-resident data.
                ReferenceView::Reflog => self.open_reflog_details(),
                ReferenceView::Tags | ReferenceView::Remotes | ReferenceView::Stash => {
                    self.open_reference_details();
                    Vec::new()
                }
            },
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

    /// Opens the reference-details overlay for the entry currently under
    /// the References panel's cursor (US-050 criterion 2), mirroring
    /// [`Self::open_commit_details`]: a no-op when the active sub-view has
    /// nothing loaded (e.g. no tags exist yet).
    fn open_reference_details(&mut self) {
        if self.reference_cursor < self.reference_len() {
            self.reference_details_open = true;
        }
    }

    /// Opens the reflog-entry details overlay for the entry currently under
    /// the References panel's cursor, while its Reflog sub-view is active
    /// (T-241/US-089 criterion 2). A no-op when nothing is loaded under the
    /// cursor, mirroring [`Self::open_reference_details`]. When the entry's
    /// commit object no longer exists, this states that clearly (US-089
    /// criterion 3) without ever attempting a read for it — reusing
    /// [`gitsail_application::GetCommit`] (via
    /// [`crate::worker::Command::LoadReflogCommit`]) exactly like the Graph
    /// panel's own commit-details overlay reuses it, never a parallel
    /// lookup. This never runs `reset` or any other mutation — inspection
    /// here is read-only, full stop (History Editing Rules #10); "go back
    /// to this state" is a deliberately separate, already-existing,
    /// explicit action ([`Action::RequestReset`], T-240), not offered from
    /// here.
    fn open_reflog_details(&mut self) -> Vec<Command> {
        let Some(entry) = self.reflog.get(self.reference_cursor).cloned() else {
            return Vec::new();
        };
        self.reflog_details_open = true;
        self.reflog_details_commit = None;
        self.reflog_details_error = None;
        if !entry.is_available() {
            self.reflog_details_error = Some(GitSailError::new(
                ErrorCode::RepositoryNotFound,
                "this reflog entry's commit object no longer exists (already expired and pruned)",
            ));
            return Vec::new();
        }
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        vec![Command::LoadReflogCommit(repo, entry.commit)]
    }

    /// Opens the amend composer (`A`, T-242/US-090 criterion 1), dispatching
    /// a non-mutating [`Command::PreviewAmend`] immediately — building the
    /// preview only ever reads `HEAD`/the staged diff, so there is nothing
    /// to confirm yet (mirrors [`Self::request_rebase_plan`]'s own "opening
    /// the composer is not itself a mutation" rationale). A no-op while
    /// already open, so a repeated `A` press never discards an in-flight
    /// edit or restarts a still-loading preview.
    fn request_start_amend(&mut self) -> Vec<Command> {
        if self.amend_open {
            return Vec::new();
        }
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        self.amend_open = true;
        self.amend_preview = None;
        self.amend_message = None;
        self.amend_error = None;
        let repo = session.repository().clone();
        vec![Command::PreviewAmend(repo)]
    }

    /// Begins confirmation for [`OperationKind::AmendCommit`] (T-242/US-090
    /// criteria 1, 2): a no-op until the preview has actually loaded — there
    /// is nothing to amend to yet, and no confirmation can honestly name the
    /// commit being replaced without it. `expected_head` is exactly the
    /// commit hash this preview observed as `HEAD`, revalidated by
    /// `RepositoryWritePort::amend_commit` immediately before it actually
    /// amends (US-090 criterion 1's own race protection — the same one
    /// `apps/desktop`'s amend flow already relies on).
    fn confirm_amend(&mut self) -> Vec<Command> {
        let Some(preview) = self.amend_preview.as_ref() else {
            return Vec::new();
        };
        self.operation.begin(OperationKind::AmendCommit {
            short_hash: preview.head.short_hash.as_str().to_string(),
            expected_head: preview.head.hash.as_str().to_string(),
        });
        Vec::new()
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
    /// [`OperationKind::prompt_label`]'s "always name the action and its
    /// target" rule for mutations.
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

    /// `Esc`'s meaning: closes whatever is topmost. A pending/running/
    /// terminal operation is handled first and on its own (T-267), because
    /// its overlay is modal and renders over everything below it — before
    /// this branch existed, an `Esc` meant for a merge confirmation sitting
    /// over the conflicts overlay closed the *conflicts* overlay instead
    /// and left the confirmation up (`conflicts_open`, `patch_export`,
    /// `sync_error` and `forge_link_error` never carried the
    /// `operation.is_idle()` guard the other branches did).
    ///
    /// Cancelling only ever touches [`Self::operation`], never the composer/
    /// plan/chooser underneath: a cancelled confirmation returns to an
    /// editable commit message, an intact rebase plan and so on, exactly as
    /// before. A second `Esc` then closes that surface.
    fn dismiss(&mut self) {
        if !self.operation.is_idle() {
            // A terminal operation overlay (`Succeeded`/`Failed`) being
            // dismissed also clears any lingering pull-outcome banner from
            // the same operation, so a stale "fast-forwarded to ..." can
            // never survive into the next one.
            if matches!(
                self.operation,
                OperationState::Succeeded(_) | OperationState::Failed(_, _)
            ) {
                self.last_pull_outcome = None;
            }
            // A no-op while `InProgress` — work already started cannot be
            // un-started (`OperationState::cancel`'s own rule), and
            // `InputContext::OperationRunning` does not even produce a
            // `Dismiss` for it.
            self.operation.cancel();
            return;
        }
        if self.help_visible {
            self.help_visible = false;
        } else if self.commit_details_open {
            self.commit_details_open = false;
        } else if self.reference_details_open {
            self.reference_details_open = false;
        } else if self.reflog_details_open {
            self.reflog_details_open = false;
            self.reflog_details_commit = None;
            self.reflog_details_error = None;
        } else if self.commit_message.is_some() {
            self.commit_message = None;
        } else if self.amend_open {
            self.amend_open = false;
            self.amend_preview = None;
            self.amend_message = None;
            self.amend_error = None;
        } else if self.rebase_plan_reword_input.is_some() {
            // Cancels only the message edit — the entry's `action` stays
            // `Reword` (client-side validation will require a message
            // before this plan can be confirmed), matching `branch_input`'s
            // own "discard the unsubmitted edit" rule below.
            self.rebase_plan_reword_input = None;
        } else if self.rebase_plan_open {
            self.rebase_plan_open = false;
            self.rebase_plan = None;
            self.rebase_plan_cursor = 0;
            self.rebase_plan_error = None;
        } else if self.reset_mode_open {
            self.reset_mode_open = false;
            self.reset_mode_cursor = 0;
            self.reset_target = None;
        } else if self.branch_input.is_some() {
            self.branch_input = None;
            self.rename_source = None;
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
        } else if self.conflicts_open {
            self.conflicts_open = false;
            self.inspected_conflict = None;
            self.conflict_error = None;
        } else if self.forge_link_error.is_some() {
            self.forge_link_error = None;
        } else if self.sync_error.is_some() {
            self.sync_error = None;
        } else {
            // Nothing is open: clear the last operation's outcome line from
            // the status bar, so `Esc` also dismisses that report once the
            // person has read it.
            self.last_operation_outcome = None;
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

    /// Opens the rename prompt for the highlighted branch (T-157/US-024),
    /// pre-filling `branch_input` with its current name so the field always
    /// shows both the previous name (as the starting text) and the new name
    /// (as whatever the person edits it to) — criterion 1. Unlike
    /// [`Self::request_checkout`]/[`Self::request_delete_branch`], this is
    /// never a no-op on the current branch: renaming the branch a person is
    /// standing on is exactly as valid as renaming any other local branch
    /// (`RepositoryWritePort::rename_branch`'s own doc).
    fn request_rename_branch(&mut self) {
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
        let name = branch.name.as_str().to_string();
        self.rename_source = Some(name.clone());
        self.branch_input = Some(name);
    }

    /// Resolves which remote fetch/pull/push should target (US-049
    /// criterion 1: remote and branch/upstream are always explicit — never
    /// an implicit, silently guessed choice). Prefers the current branch's
    /// configured upstream (`origin/main` names remote `origin`) since that
    /// is the most specific signal available; falls back to the sole
    /// configured remote when there is exactly one and no upstream is set;
    /// otherwise refuses rather than picking among several equally
    /// plausible remotes.
    fn resolve_sync_remote(&self) -> Result<String, GitSailError> {
        let Some(session) = self.session.as_ref() else {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no repository is open yet",
            ));
        };
        let current_branch = session
            .repository()
            .current_branch
            .as_ref()
            .ok_or_else(|| {
                GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "no branch is currently checked out",
                )
                .with_remediation("check out a branch before syncing with a remote")
            })?;

        if let Some(branch) = self
            .branches
            .iter()
            .find(|b| matches!(b.kind, BranchKind::Local) && &b.name == current_branch)
        {
            if let Some(upstream) = branch.upstream.as_ref() {
                if let Some((remote, _)) = upstream.as_str().split_once('/') {
                    return Ok(remote.to_string());
                }
            }
        }

        match self.remotes.len() {
            1 => Ok(self.remotes[0].name.clone()),
            0 => Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no remote is configured",
            )
            .with_remediation("add a remote (e.g. `git remote add origin <url>`) first")),
            _ => Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "the current branch has no upstream and multiple remotes are configured — cannot determine which to use",
            )
            .with_remediation("set an upstream for this branch, e.g. `git push -u <remote> <branch>`")),
        }
    }

    /// Fetches the resolved remote (US-049 criterion 1). `Safe`, so this
    /// dispatches immediately without a confirmation step, exactly like
    /// [`Self::request_toggle_stage`].
    fn request_fetch(&mut self) -> Vec<Command> {
        self.sync_error = None;
        self.last_pull_outcome = None;
        let remote = match self.resolve_sync_remote() {
            Ok(remote) => remote,
            Err(err) => {
                self.sync_error = Some(err);
                return Vec::new();
            }
        };
        let kind = OperationKind::Fetch { remote };
        self.operation.begin(kind.clone());
        self.operation.confirm();
        self.dispatch_operation(kind)
    }

    /// Starts confirmation for pulling the resolved remote's tracked branch
    /// (US-049). `Moderate`, so this always confirms first, like
    /// [`Self::request_checkout`].
    fn request_pull(&mut self) -> Vec<Command> {
        self.sync_error = None;
        self.last_pull_outcome = None;
        let Some(current_branch) = self
            .session
            .as_ref()
            .and_then(|s| s.repository().current_branch.clone())
        else {
            self.sync_error = Some(
                GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "no branch is currently checked out",
                )
                .with_remediation("check out a branch before syncing with a remote"),
            );
            return Vec::new();
        };
        let remote = match self.resolve_sync_remote() {
            Ok(remote) => remote,
            Err(err) => {
                self.sync_error = Some(err);
                return Vec::new();
            }
        };
        self.operation.begin(OperationKind::Pull {
            remote,
            branch: current_branch.as_str().to_string(),
        });
        Vec::new()
    }

    /// Starts confirmation for pushing the current branch to the resolved
    /// remote (US-049). `Moderate`, matching [`Self::request_pull`]. Never
    /// forces — see [`crate::operation::OperationKind::Push`]'s doc for why
    /// force-push is out of scope here.
    fn request_push(&mut self) -> Vec<Command> {
        self.sync_error = None;
        self.last_pull_outcome = None;
        let Some(current_branch) = self
            .session
            .as_ref()
            .and_then(|s| s.repository().current_branch.clone())
        else {
            self.sync_error = Some(
                GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "no branch is currently checked out",
                )
                .with_remediation("check out a branch before syncing with a remote"),
            );
            return Vec::new();
        };
        let remote = match self.resolve_sync_remote() {
            Ok(remote) => remote,
            Err(err) => {
                self.sync_error = Some(err);
                return Vec::new();
            }
        };
        self.operation.begin(OperationKind::Push {
            remote,
            branch: current_branch.as_str().to_string(),
        });
        Vec::new()
    }

    /// Starts confirmation for merging the highlighted reference into the
    /// current branch (T-231/US-079 criterion 1: origin — the current
    /// branch — destination and policy — a plain, non-force merge — are all
    /// shown by the resulting confirmation prompt before anything runs).
    /// Reuses exactly the Sidebar's branch search/selection mechanism
    /// [`Self::request_checkout`] already uses, rather than a bespoke
    /// picker. Unlike checkout, merging the current branch into itself is
    /// left to Git's own harmless "Already up to date" handling rather than
    /// refused as a no-op here.
    fn request_merge(&mut self) {
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
        self.last_merge_result = None;
        self.operation.begin(OperationKind::Merge {
            target: branch.name.as_str().to_string(),
        });
    }

    /// Starts confirmation for rebasing the current branch onto the
    /// highlighted reference (T-235/US-083 criterion 1: the current branch,
    /// the chosen base, and the expected rewrite are all shown by the
    /// resulting confirmation prompt before anything runs — the exact
    /// commits to be reapplied are what [`OperationKind::Rebase`]'s own
    /// target label, combined with [`Self::last_rebase_result`]'s prior
    /// state, lets a presentation layer show). Reuses exactly the same
    /// Sidebar branch search/selection mechanism [`Self::request_merge`]
    /// already uses.
    fn request_rebase(&mut self) {
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
        self.last_rebase_result = None;
        self.operation.begin(OperationKind::Rebase {
            onto: branch.name.as_str().to_string(),
        });
    }

    /// Opens the interactive rebase plan overlay for the highlighted
    /// reference (`O`, Sidebar only — reuses exactly the same Sidebar
    /// branch search/selection mechanism [`Self::request_rebase`] already
    /// uses). Unlike [`Self::request_rebase`], this dispatches
    /// [`Command::PlanRebase`] immediately rather than starting a
    /// confirmation: reading a plan never touches the working tree, the
    /// index, or any ref, so there is nothing to confirm yet (T-236/US-084
    /// criterion 1 — the plan itself, once loaded, is what gets confirmed).
    fn request_rebase_plan(&mut self) -> Vec<Command> {
        if self.focus != Panel::Sidebar {
            return Vec::new();
        }
        let Some(branch) = self
            .filtered_branches()
            .get(self.sidebar_cursor)
            .map(|b| (*b).clone())
        else {
            return Vec::new();
        };
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        self.rebase_plan_open = true;
        self.rebase_plan = None;
        self.rebase_plan_cursor = 0;
        self.rebase_plan_reword_input = None;
        self.rebase_plan_error = None;
        let repo = session.repository().clone();
        vec![Command::PlanRebase(repo, branch.name.as_str().to_string())]
    }

    /// The commit `HEAD` currently resolves to, derived from already-loaded
    /// state rather than a fresh read: [`Self::session`]'s own
    /// `status().head_state` names either the attached branch (whose tip is
    /// already loaded in [`Self::branches`]) or a detached commit directly.
    /// `None` only for an unborn `HEAD` (no commits yet) or before the
    /// first status/branches load completes. Used as the `expected_head`
    /// [`gitsail_application::write_ports::Precondition`]
    /// [`Self::confirm_reset_mode`] captures — the same commit a hard
    /// reset's reinforced confirmation names as "current HEAD" is exactly
    /// what gets revalidated immediately before the reset actually runs
    /// (US-088 criterion 3).
    fn current_head_hash(&self) -> Option<CommitHash> {
        let status = self.session.as_ref()?.status()?;
        match &status.head_state {
            HeadState::Attached { branch } => self
                .branches
                .iter()
                .find(|b| b.kind == BranchKind::Local && &b.name == branch)
                .map(|b| b.target.clone()),
            HeadState::Detached { commit } => Some(commit.clone()),
            HeadState::Unborn => None,
        }
    }

    /// Starts confirmation for cherry-picking the highlighted Graph commit
    /// onto the current branch (`x`, Graph panel only; T-238/US-086
    /// criterion 1: the exact commit and the current branch as destination
    /// are both named by the resulting confirmation prompt before anything
    /// runs). A merge commit is always cherry-picked against its first
    /// parent (the same first-parent convention this workspace's diff/graph
    /// already use for a merge commit) — the confirmation prompt names this
    /// explicitly rather than leaving it implicit (see
    /// [`crate::operation::OperationKind::CherryPick`]'s own `is_merge`
    /// field).
    fn request_cherry_pick(&mut self) {
        if self.focus != Panel::Graph {
            return;
        }
        let Some(commit) = self.selected_graph_commit().cloned() else {
            return;
        };
        self.last_cherry_pick_result = None;
        self.operation.begin(OperationKind::CherryPick {
            commit: commit.hash.as_str().to_string(),
            is_merge: commit.is_merge(),
        });
    }

    /// Starts confirmation for reverting the highlighted Graph commit (`v`,
    /// Graph panel only), mirroring [`Self::request_cherry_pick`] exactly
    /// (T-239/US-087 criterion 1).
    fn request_revert(&mut self) {
        if self.focus != Panel::Graph {
            return;
        }
        let Some(commit) = self.selected_graph_commit().cloned() else {
            return;
        };
        self.last_revert_result = None;
        self.operation.begin(OperationKind::Revert {
            commit: commit.hash.as_str().to_string(),
            is_merge: commit.is_merge(),
        });
    }

    /// Opens the reset-mode chooser overlay for the highlighted Graph commit
    /// (`z`, Graph panel only; T-240/US-088 criterion 1) — picking a mode
    /// itself never mutates anything yet, mirroring
    /// [`Self::request_rebase_plan`]'s own "opening the picker is not itself
    /// a mutation" rationale.
    fn request_reset(&mut self) {
        if self.focus != Panel::Graph {
            return;
        }
        let Some(commit) = self.selected_graph_commit() else {
            return;
        };
        self.reset_target = Some(commit.clone());
        self.reset_mode_open = true;
        self.reset_mode_cursor = 0;
    }

    /// Confirms the reset-mode chooser's highlighted mode, starting
    /// confirmation for [`OperationKind::Reset`] (US-088 criteria 1, 2): the
    /// exact target, mode, and — for `Hard` — the concrete count of
    /// uncommitted changes that would be permanently discarded (computed
    /// from the already-loaded [`gitsail_application::RepositorySession::status`],
    /// never a generic warning) are all captured here, before anything is
    /// confirmed. `expected_head` ([`Self::current_head_hash`]) is what
    /// `RepositoryWritePort::reset` revalidates immediately before actually
    /// resetting (US-088 criterion 3): a `HEAD` that moves between this
    /// moment and the final confirmation is caught there, never executed
    /// against silently.
    fn confirm_reset_mode(&mut self) {
        let Some(commit) = self.reset_target.clone() else {
            return;
        };
        let mode = match self.reset_mode_cursor {
            0 => ResetMode::Soft,
            1 => ResetMode::Mixed,
            _ => ResetMode::Hard,
        };
        let Some(expected_head) = self.current_head_hash() else {
            return;
        };
        let predicted_loss_files = self.predicted_reset_loss_file_count();
        self.reset_mode_open = false;
        self.operation.begin(OperationKind::Reset {
            target: commit.hash.as_str().to_string(),
            mode,
            expected_head: expected_head.as_str().to_string(),
            predicted_loss_files,
        });
    }

    /// Moves the highlighted plan entry one position up (`K`, T-236/US-084
    /// criterion 1's "pode reordenar"), keeping the cursor on the same
    /// entry as it moves. A no-op at the first position, and a no-op
    /// entirely while no plan is loaded yet.
    fn rebase_plan_move_entry_up(&mut self) {
        let Some(plan) = self.rebase_plan.as_mut() else {
            return;
        };
        if self.rebase_plan_cursor == 0 {
            return;
        }
        plan.entries
            .swap(self.rebase_plan_cursor, self.rebase_plan_cursor - 1);
        self.rebase_plan_cursor -= 1;
        self.rebase_plan_error = None;
    }

    /// Moves the highlighted plan entry one position down (`J`), mirroring
    /// [`Self::rebase_plan_move_entry_up`].
    fn rebase_plan_move_entry_down(&mut self) {
        let Some(plan) = self.rebase_plan.as_mut() else {
            return;
        };
        if self.rebase_plan_cursor + 1 >= plan.entries.len() {
            return;
        }
        plan.entries
            .swap(self.rebase_plan_cursor, self.rebase_plan_cursor + 1);
        self.rebase_plan_cursor += 1;
        self.rebase_plan_error = None;
    }

    /// Cycles the highlighted entry's action Pick -> Reword -> Squash ->
    /// Fixup -> Drop -> Pick (`a`, T-236/US-084 criterion 1). Landing on
    /// `Reword` immediately opens the message prompt — this task's own
    /// "reaproveite o mecanismo de input de texto" instruction — pre-filled
    /// with whatever override already exists, or the commit's own subject
    /// otherwise, a more useful starting point to edit than a blank field.
    /// Leaving `Reword` for any other action always clears
    /// `message_override`, mirroring [`RebasePlan::validate`]'s own rule
    /// that only a `Reword` entry may carry one.
    fn rebase_plan_cycle_action(&mut self) {
        let Some(plan) = self.rebase_plan.as_mut() else {
            return;
        };
        let Some(entry) = plan.entries.get_mut(self.rebase_plan_cursor) else {
            return;
        };
        entry.action = match entry.action {
            RebaseAction::Pick => RebaseAction::Reword,
            RebaseAction::Reword => RebaseAction::Squash,
            RebaseAction::Squash => RebaseAction::Fixup,
            RebaseAction::Fixup => RebaseAction::Drop,
            RebaseAction::Drop => RebaseAction::Pick,
        };
        if entry.action == RebaseAction::Reword {
            self.rebase_plan_reword_input = Some(
                entry
                    .message_override
                    .clone()
                    .unwrap_or_else(|| entry.subject.clone()),
            );
        } else {
            entry.message_override = None;
        }
        self.rebase_plan_error = None;
    }

    /// Commits the Reword prompt's current text into the highlighted
    /// entry's `message_override` (Enter within
    /// [`InputContext::RebasePlanReword`]), then returns to the plan
    /// overlay. Stores the text exactly as typed, including empty — this
    /// only echoes [`RebasePlan::validate`]'s rules for immediate feedback;
    /// it never itself decides what counts as a valid message.
    fn confirm_rebase_plan_reword(&mut self) {
        let Some(text) = self.rebase_plan_reword_input.take() else {
            return;
        };
        if let Some(plan) = self.rebase_plan.as_mut() {
            if let Some(entry) = plan.entries.get_mut(self.rebase_plan_cursor) {
                entry.message_override = Some(text);
            }
        }
    }

    /// Validates the current plan client-side (T-236/US-084 criterion 2:
    /// "plano inválido não executa"), mirroring [`RebasePlan::validate`]'s
    /// own rules exactly (squash/fixup at position 0, reword without a
    /// message, ...) so a mistake is refused *before* ever reaching
    /// [`Command::ExecuteRebasePlan`] — and, only once it passes, starts
    /// confirmation for [`OperationKind::ExecuteRebasePlan`]. Never the
    /// final authority: `RepositoryWritePort::execute_rebase_plan` always
    /// re-validates for real (and revalidates `onto`/`branch_head` against
    /// the live repository) before touching anything, so a stale plan is
    /// still refused there even if it happened to validate here a moment
    /// ago.
    fn confirm_rebase_plan(&mut self) {
        let Some(plan) = self.rebase_plan.as_ref() else {
            return;
        };
        if let Err(error) = plan.validate() {
            self.rebase_plan_error = Some(error);
            return;
        }
        self.rebase_plan_error = None;
        self.operation.begin(OperationKind::ExecuteRebasePlan {
            onto: plan.onto_revision.clone(),
            commit_count: plan.entries.len(),
        });
    }

    /// Starts confirmation to skip the current step of the pending operation
    /// (T-235/US-083 criterion 3). A no-op when skip is not offered for
    /// whatever is currently detected (e.g. a merge, which has no further
    /// step to skip past) — mirrors [`Self::request_continue_operation`]/
    /// [`Self::request_abort_operation`]'s own capability-gated convention.
    fn request_skip_operation(&mut self) {
        if !self
            .in_progress_operation
            .supports(OperationCapability::Skip)
        {
            return;
        }
        self.operation.begin(OperationKind::SkipOperation);
    }

    /// Opens or closes the conflicts overlay (`M`, T-232/US-080 criterion 1;
    /// T-233/US-081). A no-op to open when nothing currently has conflicted
    /// files — there would be nothing to show.
    fn toggle_conflicts_panel(&mut self) {
        if self.conflicts_open {
            self.conflicts_open = false;
            self.inspected_conflict = None;
            self.conflict_error = None;
            return;
        }
        if !self.in_progress_operation.has_conflicts() {
            return;
        }
        self.conflicts_open = true;
        self.conflict_cursor = 0;
        self.inspected_conflict = None;
        self.conflict_error = None;
    }

    /// Loads the base/ours/theirs sides of the conflicted file under the
    /// overlay's cursor (T-232/US-080 criterion 2).
    fn inspect_conflict(&mut self) -> Vec<Command> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let Some(file) = self
            .in_progress_operation
            .conflicted_files()
            .get(self.conflict_cursor)
        else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        let path = file.path.clone();
        vec![Command::LoadConflictSides(repo, path)]
    }

    /// Marks the conflicted file under the overlay's cursor resolved by
    /// staging its current working-tree content (T-232/US-080 criterion 3).
    /// Dispatches immediately, without a confirmation step — this task's own
    /// [`crate::operation::OperationKind`] scope deliberately does not model
    /// this mutation as a confirmable operation (mirrors
    /// [`Self::request_toggle_stage`]'s `Safe`-risk convention;
    /// `gitsail_application::MutationKind::MarkConflictResolved` is
    /// classified `Safe` for the same reason `git add` itself is).
    fn request_mark_conflict_resolved(&mut self) -> Vec<Command> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let Some(file) = self
            .in_progress_operation
            .conflicted_files()
            .get(self.conflict_cursor)
        else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        let path = file.path.clone();
        vec![Command::MarkConflictResolved(repo, path)]
    }

    /// Resolves the conflicted file under the overlay's cursor by taking
    /// `side` wholesale (T-232/US-080 criterion 3's documented
    /// binary-conflict flow). Dispatches immediately, matching
    /// [`Self::request_mark_conflict_resolved`]'s own reasoning.
    fn request_take_conflict_side(&mut self, side: ConflictSide) -> Vec<Command> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let Some(file) = self
            .in_progress_operation
            .conflicted_files()
            .get(self.conflict_cursor)
        else {
            return Vec::new();
        };
        let repo = session.repository().clone();
        let path = file.path.clone();
        vec![Command::TakeConflictSide(repo, path, side)]
    }

    /// Starts confirmation to continue the pending operation (T-233/US-081
    /// criterion 1: only offered when actually supported). A no-op
    /// otherwise — mirrors [`Self::request_checkout`]'s "no-op on the
    /// current branch" convention for an action that would not make sense
    /// right now.
    fn request_continue_operation(&mut self) {
        if !self
            .in_progress_operation
            .supports(OperationCapability::Continue)
        {
            return;
        }
        self.operation.begin(OperationKind::ContinueOperation);
    }

    /// Starts confirmation to abort the pending operation (T-233/US-081
    /// criterion 1). A no-op when abort is not offered for whatever is
    /// currently detected.
    fn request_abort_operation(&mut self) {
        if !self
            .in_progress_operation
            .supports(OperationCapability::Abort)
        {
            return;
        }
        self.operation.begin(OperationKind::AbortOperation);
    }

    /// Starts T-163/US-030's apply-patch flow (`Y`, Diff panel only,
    /// mirroring [`Self::export_patch`]'s own `y`-gating): reads the
    /// clipboard and, if it has usable text, dispatches the non-mutating
    /// preview (`git apply --check`, US-030 criterion 1). Never applies
    /// anything itself, and never even reaches [`OperationState::Confirming`]
    /// for an empty/unavailable clipboard — there is nothing to confirm.
    fn request_apply_patch(&mut self) -> Vec<Command> {
        if self.focus != Panel::Diff {
            return Vec::new();
        }
        self.patch_apply_outcome = None;
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let patch_text = match self.clipboard.get_text() {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) => {
                self.patch_apply_outcome = Some(PatchApplyOutcome::ClipboardEmpty {
                    reason: "the clipboard is empty".to_string(),
                });
                return Vec::new();
            }
            Err(reason) => {
                self.patch_apply_outcome = Some(PatchApplyOutcome::ClipboardEmpty { reason });
                return Vec::new();
            }
        };
        let repo = session.repository().clone();
        vec![Command::PreviewPatchApplication(repo, patch_text)]
    }

    /// Turns a confirmed [`OperationKind`] into the [`Command`] that
    /// actually runs it. A malformed name (only possible from a
    /// hand-typed branch name — `Branch`/`StatusEntry`-derived kinds are
    /// always well-formed) fails the operation immediately rather than ever
    /// reaching the write port (criterion 3: nothing is discarded, and
    /// nothing is attempted with data known to be invalid).
    fn dispatch_operation(&mut self, kind: OperationKind) -> Vec<Command> {
        // Bounds `pending_branch_rename`'s lifetime to the one rename it was
        // set for: any *other* confirmed operation dispatched before that
        // rename's own refresh cycle completed must never let a later,
        // unrelated `BranchesLoaded` reconcile against a stale pair (see
        // this field's own doc). The `RenameBranch` arm below re-sets it
        // immediately after, once both names are known to be valid.
        self.pending_branch_rename = None;
        // The previous operation's status-bar report belongs to the
        // operation that produced it, never to this one (T-267).
        self.last_operation_outcome = None;
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
            OperationKind::RenameBranch { old_name, new_name } => {
                match (BranchName::new(old_name), BranchName::new(new_name)) {
                    (Ok(old_name), Ok(new_name)) => {
                        self.pending_branch_rename = Some((old_name.clone(), new_name.clone()));
                        vec![Command::RenameBranch(repo, old_name, new_name)]
                    }
                    (old_result, new_result) => {
                        let err = old_result.and(new_result).unwrap_err();
                        self.operation.fail(err);
                        Vec::new()
                    }
                }
            }
            OperationKind::Fetch { remote } => vec![Command::Fetch(repo, remote)],
            OperationKind::Pull { remote, branch } => match BranchName::new(branch) {
                Ok(name) => vec![Command::Pull(repo, remote, name)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::Push { remote, branch } => match BranchName::new(branch) {
                Ok(name) => vec![Command::Push(repo, remote, name)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::ApplyPatch { .. } => {
                let patch_text = std::mem::take(&mut self.pending_patch_text).unwrap_or_default();
                vec![Command::ApplyPatch(repo, patch_text)]
            }
            OperationKind::Merge { target } => vec![Command::Merge(repo, target)],
            OperationKind::ContinueOperation => vec![Command::ContinueOperation(repo)],
            OperationKind::AbortOperation => vec![Command::AbortOperation(repo)],
            OperationKind::Rebase { onto } => vec![Command::Rebase(repo, onto)],
            OperationKind::SkipOperation => vec![Command::SkipOperation(repo)],
            OperationKind::ExecuteRebasePlan { .. } => {
                // The overlay is fully consumed the moment this actually
                // dispatches — from here on, `render_overlays` shows the
                // generic operation overlay instead (mirrors
                // `OperationKind::Merge`/`Rebase`'s own convention rather
                // than the commit composer's "stays visible" one). On
                // failure (e.g. the Core's own stale-plan revalidation
                // refusal) this is a deliberate, documented scope cut, not
                // an oversight: T-236/US-084 asks for a clear error, never
                // "reconstrução mágica" — the person presses `O` again for
                // a fresh plan rather than this silently rebuilding one.
                self.rebase_plan_open = false;
                match std::mem::take(&mut self.rebase_plan) {
                    Some(plan) => vec![Command::ExecuteRebasePlan(repo, plan)],
                    None => {
                        self.operation.fail(GitSailError::new(
                            ErrorCode::Internal,
                            "the rebase plan was lost before it could be executed",
                        ));
                        Vec::new()
                    }
                }
            }
            OperationKind::CherryPick { commit, is_merge } => match CommitHash::new(commit) {
                Ok(hash) => {
                    let merge_parent = is_merge.then_some(MergeParentPolicy::FirstParent);
                    vec![Command::CherryPick(repo, hash, merge_parent)]
                }
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::Revert { commit, is_merge } => match CommitHash::new(commit) {
                Ok(hash) => {
                    let merge_parent = is_merge.then_some(MergeParentPolicy::FirstParent);
                    vec![Command::Revert(repo, hash, merge_parent)]
                }
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::Reset {
                target,
                mode,
                expected_head,
                ..
            } => match CommitHash::new(expected_head) {
                Ok(expected_head) => vec![Command::Reset(repo, target, mode, expected_head)],
                Err(err) => {
                    self.operation.fail(err);
                    Vec::new()
                }
            },
            OperationKind::AmendCommit { expected_head, .. } => {
                match CommitHash::new(expected_head) {
                    Ok(expected_head) => {
                        let message = self.amend_message.clone().unwrap_or_default();
                        vec![Command::AmendCommit(repo, message, expected_head)]
                    }
                    Err(err) => {
                        self.operation.fail(err);
                        Vec::new()
                    }
                }
            }
        }
    }

    fn refresh_commands_for(&mut self, reason: RefreshReason) -> Vec<Command> {
        let Some(session) = self.session.as_mut() else {
            return Vec::new();
        };
        let ticket = session.begin_refresh(reason);
        let generation = session.generation();
        let repo = session.repository().clone();
        vec![
            Command::RefreshStatus(ticket, repo.clone()),
            Command::LoadBranches(generation, repo.clone()),
            Command::LoadTags(generation, repo.clone()),
            Command::LoadRemotes(generation, repo.clone()),
            Command::LoadStashEntries(generation, repo.clone()),
            Command::LoadReflog(generation, repo.clone()),
            Command::LoadInProgressOperation(generation, repo),
        ]
    }

    /// Handles the terminal regaining OS-level focus (`crossterm::event::
    /// Event::FocusGained`, enabled by `terminal::init`) — the TUI's
    /// counterpart to `apps/desktop`'s own window-focus refresh (T-234/
    /// US-082 criterion 2). A no-op while no repository is open yet, exactly
    /// like [`Self::refresh_commands_for`]'s own guard.
    ///
    /// Without this, switching away to run `git merge`/`git rebase` in
    /// another terminal and back would leave the TUI showing whatever it
    /// last loaded until the person either mutates something here or
    /// presses the manual refresh key (`r`) themselves — this makes that
    /// re-detection automatic, matching what already happens on open and
    /// after every mutation, rather than depending on stale in-memory state
    /// (T-230/US-078 criterion 2's own "never a cached/in-memory flag"
    /// applies just as much to a refresh trigger as to the read itself).
    pub fn on_focus_gained(&mut self) -> Vec<Command> {
        self.refresh_commands_for(RefreshReason::Focus)
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
                self.reference_details_open = false;
                self.reference_cursor = 0;
                self.sync_error = None;
                self.last_pull_outcome = None;
                self.patch_apply_outcome = None;
                self.pending_patch_text = None;
                self.in_progress_operation = InProgressOperation::None;
                self.conflicts_open = false;
                self.conflict_cursor = 0;
                self.inspected_conflict = None;
                self.conflict_error = None;
                self.last_merge_result = None;
                self.last_rebase_result = None;
                self.rebase_plan_open = false;
                self.rebase_plan = None;
                self.rebase_plan_cursor = 0;
                self.rebase_plan_reword_input = None;
                self.rebase_plan_error = None;
                self.last_cherry_pick_result = None;
                self.last_revert_result = None;
                self.reset_mode_open = false;
                self.reset_mode_cursor = 0;
                self.reset_target = None;
                self.reflog_details_open = false;
                self.reflog_details_commit = None;
                self.reflog_details_error = None;
                self.amend_open = false;
                self.amend_preview = None;
                self.amend_message = None;
                self.amend_error = None;

                let mut commands = vec![
                    Command::RefreshStatus(ticket, repo.clone()),
                    Command::LoadBranches(generation, repo.clone()),
                    Command::LoadTags(generation, repo.clone()),
                    Command::LoadRemotes(generation, repo.clone()),
                    Command::LoadStashEntries(generation, repo.clone()),
                    Command::LoadReflog(generation, repo.clone()),
                    Command::LoadInProgressOperation(generation, repo.clone()),
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
            // T-157/US-024 criterion 3: a branch renamed while it was the
            // session's selected branch, or highlighted in the Sidebar,
            // must still be found afterward — under its *new* name, not
            // lost. This is the one place both are reconciled, since this
            // is the first fresh list to actually contain the new name.
            if let Some((old_name, new_name)) = self.pending_branch_rename.take() {
                if let Some(session) = self.session.as_mut() {
                    if session.selection().branch.as_ref() == Some(&old_name) {
                        session.select_branch(new_name.clone());
                    }
                }
                if let Some(idx) = self
                    .filtered_branches()
                    .iter()
                    .position(|b| b.name == new_name)
                {
                    self.sidebar_cursor = idx;
                }
            }
            let max = self.filtered_branches().len().saturating_sub(1);
            self.sidebar_cursor = self.sidebar_cursor.min(max);
        }
        // A branch-load failure is left to the sidebar's existing (possibly
        // stale) list rather than promoting a secondary panel's error into
        // the whole view's error phase — the repository/status themselves
        // are what US-040 criterion 2 requires an error state for.
    }

    /// Clamps `self.reference_cursor` to the currently active reference
    /// sub-view's length, exactly like [`Self::on_branches_loaded`] clamps
    /// `sidebar_cursor` — called after any of the three lists below is
    /// (re)loaded, since whichever one is on screen right now may have
    /// shrunk.
    fn clamp_reference_cursor(&mut self) {
        let max = self.reference_len().saturating_sub(1);
        self.reference_cursor = self.reference_cursor.min(max);
    }

    /// Handles [`crate::message::Message::TagsLoaded`] (US-050), matching
    /// [`Self::on_branches_loaded`]'s staleness/failure handling: a stale or
    /// failed load leaves the existing (possibly empty) list in place
    /// rather than promoting it into the whole view's error phase.
    pub fn on_tags_loaded(&mut self, generation: u64, result: Result<Vec<Tag>, GitSailError>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        if let Ok(tags) = result {
            self.tags = tags;
            self.clamp_reference_cursor();
        }
    }

    /// Handles [`crate::message::Message::RemotesLoaded`] (US-050), matching
    /// [`Self::on_tags_loaded`].
    pub fn on_remotes_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<Remote>, GitSailError>,
    ) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        if let Ok(remotes) = result {
            self.remotes = remotes;
            self.clamp_reference_cursor();
        }
    }

    /// Handles [`crate::message::Message::StashEntriesLoaded`] (US-050),
    /// matching [`Self::on_tags_loaded`].
    pub fn on_stash_entries_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<Stash>, GitSailError>,
    ) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        if let Ok(stashes) = result {
            self.stashes = stashes;
            self.clamp_reference_cursor();
        }
    }

    /// Handles [`crate::message::Message::ReflogLoaded`] (T-241/US-089),
    /// matching [`Self::on_tags_loaded`]'s staleness/failure discipline.
    pub fn on_reflog_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<ReflogEntry>, GitSailError>,
    ) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        if let Ok(entries) = result {
            self.reflog = entries;
            self.clamp_reference_cursor();
        }
    }

    /// Handles [`crate::message::Message::ReflogCommitLoaded`] (T-241/US-089
    /// criterion 2), discarding a result for an entry no longer under the
    /// cursor — mirrors [`Self::on_conflict_sides_loaded`]'s own hash/path
    /// tagged staleness discipline.
    pub fn on_reflog_commit_loaded(
        &mut self,
        hash: CommitHash,
        result: Result<Commit, GitSailError>,
    ) {
        if !self.reflog_details_open {
            return;
        }
        let Some(current) = self.reflog.get(self.reference_cursor) else {
            return;
        };
        if current.commit != hash {
            return;
        }
        match result {
            Ok(commit) => {
                self.reflog_details_commit = Some(commit);
                self.reflog_details_error = None;
            }
            Err(error) => {
                self.reflog_details_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::AmendPreviewed`] (T-242/US-090
    /// criterion 1), matching [`Self::on_rebase_plan_loaded`]'s own shape:
    /// discarded when the composer has since been dismissed, and the
    /// message is pre-filled from `HEAD`'s current subject/body exactly
    /// like `apps/desktop/src/stores/amend.ts`'s own `loadPreview` (never a
    /// different prefill rule for the TUI).
    pub fn on_amend_previewed(&mut self, result: Result<AmendPreview, GitSailError>) {
        if !self.amend_open {
            return;
        }
        match result {
            Ok(preview) => {
                self.amend_message = Some(if preview.head.body.trim().is_empty() {
                    preview.head.subject.clone()
                } else {
                    format!("{}\n\n{}", preview.head.subject, preview.head.body)
                });
                self.amend_error = None;
                self.amend_preview = Some(preview);
            }
            Err(error) => {
                self.amend_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::AmendCommitFinished`] (T-242/
    /// US-090). Success clears the composer and refreshes the commit graph
    /// (US-090 criterion 3: HEAD's identity changed) alongside status —
    /// mirroring [`Self::on_commit_created`], extended with the commit-graph
    /// restart no other mutation here performs (a deliberate, narrow
    /// addition: amend is the one operation in this crate that rewrites the
    /// Graph panel's own tip commit in place, so leaving it unrefreshed
    /// would show a stale hash/subject for `HEAD` until the next unrelated
    /// refresh). Failure preserves the typed message and the loaded preview
    /// exactly as they were (criterion 3) — `amend_commit` itself never
    /// touches the index/working tree unless it actually succeeds, so
    /// staged changes are equally untouched by construction.
    pub fn on_amend_finished(&mut self, result: Result<CommitHash, GitSailError>) -> Vec<Command> {
        match result {
            Ok(_hash) => {
                self.succeed_operation();
                self.amend_open = false;
                self.amend_preview = None;
                self.amend_message = None;
                self.amend_error = None;
                let mut commands = self.refresh_commands_for(RefreshReason::AfterMutation);
                let filter =
                    parse_commit_search(self.active_commit_filter.as_deref().unwrap_or(""));
                commands.extend(self.restart_commit_graph(filter));
                commands
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::InProgressOperationLoaded`]
    /// (T-230/US-078, presentation side of T-231/T-233), matching
    /// [`Self::on_tags_loaded`]'s staleness discipline. Never inferred from
    /// GitSail's own last action — always this freshly re-read state (US-078
    /// criterion 2; US-081 criterion 3) — so a merge/rebase/... started in
    /// another terminal, or the real aftermath of a continue/abort this
    /// session just ran, is always what ends up shown. Closes the conflicts
    /// overlay and clamps its cursor once the underlying operation/conflict
    /// list has actually changed, so it can never keep pointing past the end
    /// of a shorter list or linger open once nothing is pending anymore.
    pub fn on_in_progress_operation_loaded(
        &mut self,
        generation: u64,
        result: Result<InProgressOperation, GitSailError>,
    ) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.generation() != generation {
            return;
        }
        let Ok(operation) = result else {
            return;
        };
        if operation.is_none() {
            self.conflicts_open = false;
            self.conflict_cursor = 0;
            self.inspected_conflict = None;
            self.conflict_error = None;
        } else {
            let len = operation.conflicted_files().len();
            if len == 0 {
                self.conflict_cursor = 0;
            } else if self.conflict_cursor >= len {
                self.conflict_cursor = len - 1;
            }
        }
        self.in_progress_operation = operation;
    }

    /// Handles [`crate::message::Message::MergeFinished`] (T-231/US-079).
    /// Success records the [`MergeResult`] (criterion 2: fast-forward,
    /// merge-commit and conflict are always shown as three distinct,
    /// explicit outcomes — never collapsed into a bare success, and a
    /// conflict is never reported as one either) and refreshes, which is
    /// also what picks up the resulting `InProgressOperation::Merge` when
    /// the result was [`MergeResult::Conflict`] (criterion 3). A refused
    /// merge (e.g. another operation already in progress, or local changes
    /// that would be overwritten) moves to `Failed` with its message,
    /// exactly like [`Self::on_pull_finished`].
    pub fn on_merge_finished(&mut self, result: Result<MergeResult, GitSailError>) -> Vec<Command> {
        match result {
            Ok(outcome) => {
                self.succeed_operation();
                self.last_merge_result = Some(outcome);
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::RebaseFinished`] (T-235/US-083).
    /// Success records the [`RebaseResult`] (criterion 3: completion and
    /// conflict are always shown as two distinct, explicit outcomes — never
    /// collapsed into a bare success, and a conflict is never reported as
    /// one either) and refreshes, which is also what picks up the resulting
    /// `InProgressOperation::Rebase` when the result was
    /// [`RebaseResult::Conflict`]. A refused rebase (e.g. another operation
    /// already in progress, or a dirty working tree) moves to `Failed` with
    /// its message, exactly like [`Self::on_merge_finished`].
    pub fn on_rebase_finished(
        &mut self,
        result: Result<RebaseResult, GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(outcome) => {
                self.succeed_operation();
                self.last_rebase_result = Some(outcome);
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::CherryPickFinished`] (T-238/
    /// US-086). Success records the [`CherryPickResult`] (criterion 3:
    /// applying, a conflict, and an empty "already applied" result are
    /// always three distinct, explicit outcomes) and refreshes, which is
    /// also what picks up the resulting `InProgressOperation::CherryPick`
    /// when the result was `Conflict` or `Empty` — both leave a pending
    /// cherry-pick recoverable via skip/abort, mirroring
    /// [`Self::on_merge_finished`]'s own reasoning exactly.
    pub fn on_cherry_pick_finished(
        &mut self,
        result: Result<CherryPickResult, GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(outcome) => {
                self.succeed_operation();
                self.last_cherry_pick_result = Some(outcome);
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::RevertFinished`] (T-239/US-087),
    /// mirroring [`Self::on_cherry_pick_finished`] exactly.
    pub fn on_revert_finished(
        &mut self,
        result: Result<RevertResult, GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(outcome) => {
                self.succeed_operation();
                self.last_revert_result = Some(outcome);
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::RebasePlanLoaded`] (T-236/US-084
    /// criterion 1). A load failure is shown inline in the overlay (already
    /// opened by [`Self::request_rebase_plan`]) rather than through
    /// [`OperationState`], mirroring [`Self::on_conflict_sides_loaded`]'s own
    /// "side-channel error" convention — nothing was ever confirmed here,
    /// so there is no operation to fail.
    ///
    /// A result arriving after the overlay was already dismissed (`Esc`) is
    /// discarded outright: it must never resurrect a plan the person already
    /// walked away from.
    pub fn on_rebase_plan_loaded(&mut self, result: Result<RebasePlan, GitSailError>) {
        if !self.rebase_plan_open {
            return;
        }
        match result {
            Ok(plan) => {
                self.rebase_plan_cursor = 0;
                self.rebase_plan_error = None;
                self.rebase_plan = Some(plan);
            }
            Err(error) => {
                self.rebase_plan_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::ConflictSidesLoaded`] (T-232/
    /// US-080 criterion 2), discarding a result for a conflicted file no
    /// longer under the cursor — mirrors [`Self::on_diff_loaded`]'s
    /// staleness discipline, keyed by path (conflict inspection has no
    /// monotonic request id of its own).
    pub fn on_conflict_sides_loaded(
        &mut self,
        path: PathBuf,
        result: Result<ConflictSides, GitSailError>,
    ) {
        let Some(current) = self
            .in_progress_operation
            .conflicted_files()
            .get(self.conflict_cursor)
        else {
            return;
        };
        if current.path != path {
            return;
        }
        match result {
            Ok(sides) => {
                self.inspected_conflict = Some(sides);
                self.conflict_error = None;
            }
            Err(error) => {
                self.conflict_error = Some(error);
            }
        }
    }

    /// Handles [`crate::message::Message::ConflictResolutionFinished`]
    /// (T-232/US-080 criterion 3): a `MarkConflictResolved`/
    /// `TakeConflictSide` [`Command`] completed. Success refreshes — which
    /// re-reads the conflicted-file list, so a resolved file disappears from
    /// the overlay only once Git's own index genuinely reports it resolved,
    /// never presumed here. Failure is shown inline in the overlay rather
    /// than through [`OperationState`] (these two mutations dispatch without
    /// a confirmation step — see [`Self::request_mark_conflict_resolved`]'s
    /// doc).
    pub fn on_conflict_resolution_finished(
        &mut self,
        result: Result<(), GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(()) => {
                self.inspected_conflict = None;
                self.conflict_error = None;
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.conflict_error = Some(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::OperationResolutionFinished`]
    /// (T-233/US-081): a `ContinueOperation`/`AbortOperation` [`Command`]
    /// completed. This message alone only means the Git command itself
    /// exited successfully — it is never treated as "the operation is now
    /// fully concluded" on its own; the refresh this triggers reloads
    /// [`Self::in_progress_operation`] fresh, and *that* subsequent result is
    /// what actually tells the overlay whether the operation is really gone
    /// (US-081 criterion 3: "resultado real é reinspecionado"). A refused
    /// continue (e.g. conflicts remain) or abort (e.g. nothing pending)
    /// moves to `Failed` with its message, exactly like
    /// [`Self::on_pull_finished`].
    pub fn on_operation_resolution_finished(
        &mut self,
        result: Result<(), GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(()) => {
                self.succeed_operation();
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
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
    /// Records a successful completion and closes the overlay with it
    /// (T-267): the operation moves to `Succeeded` — so a late or duplicate
    /// completion message still cannot overwrite a state the caller has
    /// moved past ([`OperationState::succeed`]'s own in-progress guard) —
    /// its kind is handed to the status bar through
    /// [`Self::last_operation_outcome`], and the state returns to `Idle`.
    ///
    /// Every completion handler in this file funnels through here, so
    /// "success closes itself, failure stays until dismissed" is one rule in
    /// one place rather than a convention each handler could drift from.
    /// This generalizes what used to be a `Safe`-risk-only special case
    /// (stage/unstage returned straight to `Idle`, everything else left a
    /// `Succeeded` popup on screen — which, before this same task, no key
    /// could dismiss).
    fn succeed_operation(&mut self) {
        self.operation.succeed();
        if let OperationState::Succeeded(kind) = &self.operation {
            self.last_operation_outcome = Some(kind.clone());
        }
        self.operation.cancel();
    }

    /// Like every other completion handler here, success goes through
    /// [`Self::succeed_operation`], which closes the overlay and leaves the
    /// report to the status bar (T-267).
    pub fn on_operation_finished(&mut self, result: Result<(), GitSailError>) -> Vec<Command> {
        match result {
            Ok(()) => {
                self.succeed_operation();
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
                self.succeed_operation();
                self.commit_message = None;
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::PullFinished`] (US-049). Success
    /// records the [`PullOutcome`] (criterion 1: "already up to date" and
    /// "fast-forwarded" are shown explicitly, never collapsed into a bare
    /// success) and refreshes; a refused divergence — reported as an
    /// ordinary `Err` per `RepositoryWritePort::pull`'s fixed
    /// fast-forward-only policy — moves to `Failed` with its message,
    /// exactly like [`Self::on_operation_finished`], and never merges,
    /// rebases, or force-integrates anything on its own (criterion 3).
    pub fn on_pull_finished(&mut self, result: Result<PullOutcome, GitSailError>) -> Vec<Command> {
        match result {
            Ok(outcome) => {
                self.succeed_operation();
                self.last_pull_outcome = Some(outcome);
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.operation.fail(error);
                Vec::new()
            }
        }
    }

    /// Handles [`crate::message::Message::PatchPreviewed`] (T-163/US-030
    /// criterion 1). A supported preview starts [`OperationState::Confirming`]
    /// with the concrete affected-file count and holds onto the exact patch
    /// text for the confirmed apply; an unsupported preview (US-030
    /// criterion 2: malformed, out-of-repository path, or stale context) —
    /// or a hard failure building it at all — is reported as a clear
    /// banner and never reaches confirmation.
    pub fn on_patch_previewed(
        &mut self,
        result: Result<PatchPreview, GitSailError>,
        patch_text: String,
    ) {
        match result {
            Ok(preview) if preview.supported => {
                let affected_file_count = preview.affected_files.len();
                self.pending_patch_text = Some(patch_text);
                self.operation.begin(OperationKind::ApplyPatch {
                    affected_file_count,
                });
            }
            Ok(preview) => {
                self.patch_apply_outcome = Some(PatchApplyOutcome::Rejected {
                    reason: preview
                        .rejection_reason
                        .unwrap_or_else(|| "the patch cannot be applied".to_string()),
                });
            }
            Err(error) => {
                self.patch_apply_outcome = Some(PatchApplyOutcome::Rejected {
                    reason: error.message().to_string(),
                });
            }
        }
    }

    /// Handles [`crate::message::Message::PatchApplied`] (T-163/US-030).
    /// Success reports exactly the applied files (criterion 3) and
    /// refreshes; failure — e.g. the file changed again between preview and
    /// confirmation — moves to [`OperationState::Failed`] and reports a
    /// banner, never claiming a rollback Git does not actually guarantee.
    pub fn on_patch_applied(
        &mut self,
        result: Result<ApplyPatchResult, GitSailError>,
    ) -> Vec<Command> {
        match result {
            Ok(applied) => {
                self.succeed_operation();
                self.patch_apply_outcome = Some(PatchApplyOutcome::Applied {
                    affected_files: applied.applied_files,
                });
                self.refresh_commands_for(RefreshReason::AfterMutation)
            }
            Err(error) => {
                self.patch_apply_outcome = Some(PatchApplyOutcome::Failed {
                    reason: error.message().to_string(),
                });
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
        FileContentAtRevision, FileDiff, FileStatusCode, RepositoryId,
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

        fn file_content(
            &self,
            _repo: &Repository,
            _revision: &CommitHash,
            _path: &Path,
        ) -> Result<FileContentAtRevision, GitSailError> {
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

        let (ticket, repo_for_status) = open_commands
            .iter()
            .find_map(|c| match c {
                Command::RefreshStatus(t, r) => Some((*t, r.clone())),
                _ => None,
            })
            .expect("a RefreshStatus command");
        assert!(
            open_commands
                .iter()
                .any(|c| matches!(c, Command::LoadBranches(_, _))),
            "opening a repository must also request its branches"
        );
        assert!(
            open_commands
                .iter()
                .any(|c| matches!(c, Command::LoadCommitGraph(_, _, _))),
            "opening a repository must also request its commit graph"
        );

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
    fn focus_gained_is_a_no_op_without_an_open_session() {
        let (mut app, _port) = new_app();
        assert!(app.on_focus_gained().is_empty());
    }

    #[test]
    fn focus_gained_re_detects_the_in_progress_operation_alongside_the_rest_of_the_refresh() {
        // T-234/US-082 criterion 2: regaining the terminal's OS-level focus
        // must re-run the exact same refresh set `Action::Refresh` (the
        // manual key) and a post-mutation refresh already run — including
        // `LoadInProgressOperation`, so an operation started in another
        // terminal while this one merely sat unfocused is picked up without
        // requiring the person to press `r` themselves.
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));

        let commands = app.on_focus_gained();

        assert!(
            matches!(
                commands.as_slice(),
                [
                    Command::RefreshStatus(_, _),
                    Command::LoadBranches(_, _),
                    Command::LoadTags(_, _),
                    Command::LoadRemotes(_, _),
                    Command::LoadStashEntries(_, _),
                    Command::LoadReflog(_, _),
                    Command::LoadInProgressOperation(_, _),
                ]
            ),
            "unexpected commands: {commands:?}"
        );
        let generation_after = app.session().unwrap().generation();
        assert!(
            matches!(commands.last(), Some(Command::LoadInProgressOperation(g, _)) if *g == generation_after),
            "the in-progress-operation reload must be tagged with the session's new generation"
        );
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
        let (ticket, repo) = open_commands
            .iter()
            .find_map(|c| match c {
                Command::RefreshStatus(t, r) => Some((*t, r.clone())),
                _ => None,
            })
            .expect("a RefreshStatus command");

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

    // -- T-157/US-024: rename branch --------------------------------------

    #[test]
    fn renaming_a_branch_pre_fills_the_previous_name_then_confirms_then_dispatches() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![
                sample_branch("main", true),
                sample_branch("develop", false),
            ]),
        );
        app.update(Action::MoveDown); // highlight "develop"

        app.update(Action::StartRenameBranch);
        assert_eq!(
            app.branch_input(),
            Some("develop"),
            "the prompt must pre-fill the branch's previous name (criterion 1)"
        );
        assert_eq!(app.input_context(), InputContext::RenameBranch);

        // Edit the pre-filled text down to a new name.
        for _ in 0.."develop".len() {
            app.update(Action::BranchNameBackspace);
        }
        for c in "develop-renamed".chars() {
            app.update(Action::BranchNameInput(c));
        }

        app.update(Action::Activate);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::RenameBranch { .. })
        ));
        assert!(
            app.branch_input().is_none(),
            "the prompt closes once both names are committed to the confirmation"
        );

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::RenameBranch(_, old_name, new_name)] => {
                assert_eq!(old_name.as_str(), "develop");
                assert_eq!(new_name.as_str(), "develop-renamed");
            }
            other => panic!("expected exactly one RenameBranch command, got {other:?}"),
        }
    }

    #[test]
    fn renaming_the_current_branch_is_allowed_unlike_checkout_and_delete() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![sample_branch("main", true)]),
        );

        app.update(Action::StartRenameBranch);

        assert_eq!(
            app.branch_input(),
            Some("main"),
            "renaming the current branch must never be a no-op"
        );
    }

    #[test]
    fn dismissing_a_pending_rename_clears_the_prompt_without_dispatching_anything() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![sample_branch("main", true)]),
        );

        app.update(Action::StartRenameBranch);
        app.update(Action::BranchNameInput('x'));
        app.update(Action::Dismiss);

        assert!(app.branch_input().is_none());
        assert_eq!(
            app.input_context(),
            InputContext::Normal,
            "cancelling a rename must never leave a stale rename-source lingering"
        );

        // A fresh create-branch prompt right after must behave like an
        // ordinary create, never accidentally resuming the cancelled rename.
        app.update(Action::StartCreateBranch);
        assert_eq!(app.input_context(), InputContext::BranchName);
    }

    #[test]
    fn a_successful_rename_of_the_selected_current_branch_follows_selection_and_cursor_to_the_new_name(
    ) {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![
                sample_branch("develop", false),
                sample_branch("main", true),
            ]),
        );
        // Select "main" explicitly (Sidebar Enter), matching the session
        // selection this reconciliation must follow.
        app.update(Action::MoveDown); // "develop" -> "main"
        app.update(Action::Activate);
        assert_eq!(
            app.session()
                .unwrap()
                .selection()
                .branch
                .as_ref()
                .map(BranchName::as_str),
            Some("main")
        );

        app.update(Action::StartRenameBranch);
        for _ in 0.."main".len() {
            app.update(Action::BranchNameBackspace);
        }
        for c in "trunk".chars() {
            app.update(Action::BranchNameInput(c));
        }
        app.update(Action::Activate); // -> Confirming
        app.update(Action::Activate); // -> InProgress, dispatches RenameBranch

        app.on_operation_finished(Ok(()));
        // A success closes its own overlay and reports itself through the
        // status bar instead (T-267).
        assert!(app.operation().is_idle());
        assert!(matches!(
            app.last_operation_outcome(),
            Some(OperationKind::RenameBranch { .. })
        ));

        // The refresh this success triggered reports the renamed branch
        // under its new name, still current.
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![
                sample_branch("develop", false),
                sample_branch("trunk", true),
            ]),
        );

        assert_eq!(
            app.session()
                .unwrap()
                .selection()
                .branch
                .as_ref()
                .map(BranchName::as_str),
            Some("trunk"),
            "the selection must follow the rename to the new name, not be lost"
        );
        let cursor_branch = app
            .filtered_branches()
            .get(app.sidebar_cursor())
            .map(|b| b.name.as_str());
        assert_eq!(
            cursor_branch,
            Some("trunk"),
            "the sidebar cursor must still highlight the renamed branch"
        );
    }

    #[test]
    fn a_failed_rename_never_refreshes_and_never_leaves_a_stale_pending_reconciliation() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![sample_branch("main", true)]),
        );

        app.update(Action::StartRenameBranch);
        app.update(Action::BranchNameInput('x'));
        app.update(Action::Activate); // Confirming
        let commands = app.update(Action::Activate); // InProgress, dispatches
        assert!(!commands.is_empty());

        let refresh_commands = app.on_operation_finished(Err(GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "a branch with that name already exists",
        )));
        assert!(
            refresh_commands.is_empty(),
            "a failed rename must never refresh (criterion 2: nothing discarded)"
        );
        assert!(matches!(
            app.operation(),
            OperationState::Failed(OperationKind::RenameBranch { .. }, _)
        ));

        // A later, unrelated branches load (e.g. a manual refresh) must
        // never be misinterpreted as this failed rename's own reconciliation.
        app.on_branches_loaded(
            app.session().unwrap().generation(),
            Ok(vec![sample_branch("main", true)]),
        );
        assert_eq!(app.sidebar_cursor(), 0);
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
            commands
                .iter()
                .any(|c| matches!(c, Command::RefreshStatus(_, _))),
            "success must refresh status"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::LoadBranches(_, _))),
            "success must refresh branches"
        );
        assert!(app.commit_message().is_none());
        // A success closes its own overlay (T-267): the operation returns
        // to `Idle` and the completed kind is what the status bar reports.
        assert!(app.operation().is_idle());
        assert!(app.last_operation_outcome().is_some());
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
        assert_eq!(
            saved,
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n"
        );
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

    // -- T-163/US-030: apply a patch ---------------------------------------

    fn sample_patch_preview(supported: bool) -> PatchPreview {
        PatchPreview {
            affected_files: vec![PathBuf::from("a.txt")],
            supported,
            rejection_reason: if supported {
                None
            } else {
                Some("the patch no longer applies to the current file content".to_string())
            },
        }
    }

    #[test]
    fn apply_patch_reads_the_clipboard_and_dispatches_a_preview() {
        let clipboard = Arc::new(FakeClipboard {
            contents: std::sync::Mutex::new(Some("--- a/a.txt\n+++ b/a.txt\n".to_string())),
            ..Default::default()
        });
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        let commands = app.update(Action::RequestApplyPatch);

        match commands.as_slice() {
            [Command::PreviewPatchApplication(_, patch_text)] => {
                assert_eq!(patch_text, "--- a/a.txt\n+++ b/a.txt\n");
            }
            other => panic!("expected exactly one PreviewPatchApplication command, got {other:?}"),
        }
        assert!(
            app.operation().is_idle(),
            "the preview alone must never start a confirmation"
        );
    }

    #[test]
    fn apply_patch_outside_the_diff_panel_is_a_no_op() {
        let clipboard = Arc::new(FakeClipboard {
            contents: std::sync::Mutex::new(Some("a patch".to_string())),
            ..Default::default()
        });
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        open_and_load_dirty_status(&mut app);
        assert_eq!(app.focus(), Panel::Sidebar);

        let commands = app.update(Action::RequestApplyPatch);

        assert!(commands.is_empty());
        assert!(app.patch_apply_outcome().is_none());
    }

    #[test]
    fn apply_patch_with_an_empty_clipboard_reports_clipboard_empty_without_a_command() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        let commands = app.update(Action::RequestApplyPatch);

        assert!(commands.is_empty());
        assert!(matches!(
            app.patch_apply_outcome(),
            Some(PatchApplyOutcome::ClipboardEmpty { .. })
        ));
    }

    #[test]
    fn a_supported_preview_starts_confirmation_with_the_concrete_affected_file_count() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        app.on_patch_previewed(Ok(sample_patch_preview(true)), "the patch text".to_string());

        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::ApplyPatch {
                affected_file_count: 1
            })
        ));
        assert!(
            app.patch_apply_outcome().is_none(),
            "a supported preview is not itself a banner-worthy outcome"
        );

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::ApplyPatch(_, patch_text)] => assert_eq!(patch_text, "the patch text"),
            other => panic!("expected exactly one ApplyPatch command, got {other:?}"),
        }
    }

    #[test]
    fn an_unsupported_preview_is_rejected_with_a_clear_reason_and_never_reaches_confirmation() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());

        app.on_patch_previewed(
            Ok(sample_patch_preview(false)),
            "the patch text".to_string(),
        );

        assert!(
            app.operation().is_idle(),
            "a rejected preview must never start a confirmation"
        );
        match app.patch_apply_outcome() {
            Some(PatchApplyOutcome::Rejected { reason }) => assert!(!reason.is_empty()),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn a_successful_apply_reports_the_applied_files_and_refreshes() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());
        app.on_patch_previewed(Ok(sample_patch_preview(true)), "the patch text".to_string());
        app.update(Action::Activate);

        let commands = app.on_patch_applied(Ok(ApplyPatchResult {
            applied_files: vec![PathBuf::from("a.txt")],
        }));

        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::RefreshStatus(_, _))),
            "a successful apply must refresh"
        );
        match app.patch_apply_outcome() {
            Some(PatchApplyOutcome::Applied { affected_files }) => {
                assert_eq!(affected_files, &[PathBuf::from("a.txt")]);
            }
            other => panic!("expected Applied, got {other:?}"),
        }
        // A success closes its own overlay (T-267): the operation returns
        // to `Idle` and the completed kind is what the status bar reports.
        assert!(app.operation().is_idle());
        assert!(app.last_operation_outcome().is_some());
    }

    #[test]
    fn a_failed_confirmed_apply_reports_failure_without_claiming_a_rollback() {
        let clipboard = Arc::new(FakeClipboard::default());
        let (mut app, _port) = new_app_with_clipboard(clipboard);
        app_with_diff_focused(&mut app, modified_a_txt_diff());
        app.on_patch_previewed(Ok(sample_patch_preview(true)), "the patch text".to_string());
        app.update(Action::Activate);

        let commands = app.on_patch_applied(Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "the patch no longer applies to the current file content",
        )));

        assert!(commands.is_empty(), "a failed apply must never refresh");
        match app.patch_apply_outcome() {
            Some(PatchApplyOutcome::Failed { reason }) => {
                assert!(!reason.to_lowercase().contains("rollback"));
                assert!(!reason.is_empty());
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
    }

    // -- US-049: sync with a remote ---------------------------------------

    fn sample_remote(name: &str) -> Remote {
        Remote {
            name: name.to_string(),
            fetch_url: gitsail_domain::RemoteUrl::new(format!("https://example.test/{name}.git")),
            push_url: gitsail_domain::RemoteUrl::new(format!("https://example.test/{name}.git")),
        }
    }

    fn open_with_branches_and_remotes(app: &mut App, branches: Vec<Branch>, remotes: Vec<Remote>) {
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_branches_loaded(generation, Ok(branches));
        app.on_remotes_loaded(generation, Ok(remotes));
    }

    #[test]
    fn fetch_with_a_single_configured_remote_dispatches_without_confirmation() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin")],
        );

        let commands = app.update(Action::RequestFetch);
        match commands.as_slice() {
            [Command::Fetch(_, remote)] => assert_eq!(remote, "origin"),
            other => panic!("expected exactly one Fetch command, got {other:?}"),
        }
        assert!(
            matches!(
                app.operation(),
                OperationState::InProgress(OperationKind::Fetch { .. })
            ),
            "Fetch is Safe and must skip the Confirming step"
        );
        assert!(app.sync_error().is_none());
    }

    #[test]
    fn fetch_with_no_remote_configured_reports_a_clear_error_without_dispatching() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(&mut app, vec![sample_branch("main", true)], vec![]);

        let commands = app.update(Action::RequestFetch);
        assert!(commands.is_empty(), "nothing must be dispatched");
        assert!(app.operation().is_idle());
        assert!(
            app.sync_error()
                .map(|e| e.message().contains("no remote"))
                .unwrap_or(false),
            "the error must explain no remote is configured, got {:?}",
            app.sync_error()
        );
    }

    #[test]
    fn fetch_with_multiple_remotes_and_no_upstream_refuses_to_guess() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin"), sample_remote("upstream")],
        );

        let commands = app.update(Action::RequestFetch);
        assert!(
            commands.is_empty(),
            "an ambiguous remote must never be guessed"
        );
        assert!(app.sync_error().is_some());
    }

    #[test]
    fn fetch_prefers_the_current_branchs_upstream_remote_over_a_guess() {
        let (mut app, _port) = new_app();
        let mut main = sample_branch("main", true);
        main.upstream = Some(BranchName::new("upstream/main").unwrap());
        open_with_branches_and_remotes(
            &mut app,
            vec![main],
            vec![sample_remote("origin"), sample_remote("upstream")],
        );

        let commands = app.update(Action::RequestFetch);
        match commands.as_slice() {
            [Command::Fetch(_, remote)] => assert_eq!(remote, "upstream"),
            other => panic!("expected exactly one Fetch command, got {other:?}"),
        }
    }

    #[test]
    fn pull_confirms_then_dispatches_against_the_resolved_remote_and_current_branch() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin")],
        );

        app.update(Action::RequestPull);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::Pull { .. })
        ));

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::Pull(_, remote, branch)] => {
                assert_eq!(remote, "origin");
                assert_eq!(branch.as_str(), "main");
            }
            other => panic!("expected exactly one Pull command, got {other:?}"),
        }
    }

    #[test]
    fn push_confirms_then_dispatches_against_the_resolved_remote_and_current_branch() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin")],
        );

        app.update(Action::RequestPush);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::Push { .. })
        ));

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::Push(_, remote, branch)] => {
                assert_eq!(remote, "origin");
                assert_eq!(branch.as_str(), "main");
            }
            other => panic!("expected exactly one Push command, got {other:?}"),
        }
    }

    #[test]
    fn a_successful_pull_records_its_outcome_and_refreshes() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin")],
        );
        app.update(Action::RequestPull);
        app.update(Action::Activate);

        let commands = app.on_pull_finished(Ok(PullOutcome::AlreadyUpToDate));
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::RefreshStatus(_, _))),
            "a successful pull must refresh"
        );
        assert_eq!(app.last_pull_outcome(), Some(&PullOutcome::AlreadyUpToDate));
        // A success closes its own overlay (T-267): the operation returns
        // to `Idle` and the completed kind is what the status bar reports.
        assert!(app.operation().is_idle());
        assert!(app.last_operation_outcome().is_some());
    }

    #[test]
    fn a_rejected_pull_never_merges_rebases_or_refreshes() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true)],
            vec![sample_remote("origin")],
        );
        app.update(Action::RequestPull);
        app.update(Action::Activate);

        let commands = app.on_pull_finished(Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "branches have diverged",
        )));
        assert!(commands.is_empty(), "a rejected pull must never refresh");
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
        assert!(app.last_pull_outcome().is_none());
    }

    // -- US-050: inspect tags, remotes and stash --------------------------

    fn sample_tag(name: &str) -> Tag {
        Tag {
            name: name.to_string(),
            target: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            kind: gitsail_domain::TagKind::Lightweight,
        }
    }

    fn sample_stash(index: u32) -> Stash {
        Stash {
            index,
            commit: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            message: format!("WIP on main: stash {index}"),
            date: gitsail_domain::GitTimestamp::new(0, 0),
        }
    }

    #[test]
    fn tags_remotes_and_stash_are_listed_after_loading() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();

        app.on_tags_loaded(generation, Ok(vec![sample_tag("v1.0")]));
        app.on_remotes_loaded(generation, Ok(vec![sample_remote("origin")]));
        app.on_stash_entries_loaded(generation, Ok(vec![sample_stash(0)]));

        assert_eq!(app.tags().len(), 1);
        assert_eq!(app.remotes().len(), 1);
        assert_eq!(app.stashes().len(), 1);
    }

    #[test]
    fn a_stale_tags_result_is_discarded() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let stale_generation = app.session().unwrap().generation();

        app.update(Action::Refresh);

        app.on_tags_loaded(stale_generation, Ok(vec![sample_tag("stale-only")]));

        assert!(
            app.tags().is_empty(),
            "a tags result computed for an old generation must not populate the panel"
        );
    }

    #[test]
    fn empty_reference_lists_are_a_legitimate_state_not_an_error() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();

        app.on_tags_loaded(generation, Ok(vec![]));
        app.on_remotes_loaded(generation, Ok(vec![]));
        app.on_stash_entries_loaded(generation, Ok(vec![]));

        assert!(app.tags().is_empty());
        assert!(app.remotes().is_empty());
        assert!(app.stashes().is_empty());
    }

    // -- T-243/US-101: open a detected forge remote in the browser ---------

    #[test]
    fn opening_in_browser_with_no_recognized_forge_remote_is_a_silent_no_op() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_remotes_loaded(generation, Ok(vec![sample_remote("origin")]));

        assert!(app.forge_link_target().is_none());
        let commands = app.update(Action::RequestOpenForgeLink);
        assert!(commands.is_empty(), "no command must be dispatched");
        assert!(
            app.forge_link_error().is_none(),
            "an unrecognized remote is not an error (US-101 criterion 3)"
        );
    }

    #[test]
    fn opening_in_browser_with_a_github_remote_resolves_the_repository_link() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        let mut remote = sample_remote("origin");
        remote.fetch_url = gitsail_domain::RemoteUrl::new("https://github.com/org/repo.git");
        remote.push_url = remote.fetch_url.clone();
        app.on_remotes_loaded(generation, Ok(vec![remote]));

        assert_eq!(
            app.forge_link_target().as_deref(),
            Some("https://github.com/org/repo")
        );

        let commands = app.update(Action::RequestOpenForgeLink);
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            Command::OpenUrl(url) if url == "https://github.com/org/repo"
        ));
    }

    #[test]
    fn opening_in_browser_targets_the_highlighted_branch_when_sidebar_is_focused() {
        let (mut app, _port) = new_app();
        let mut remote = sample_remote("origin");
        remote.fetch_url = gitsail_domain::RemoteUrl::new("git@github.com:org/repo.git");
        remote.push_url = remote.fetch_url.clone();
        open_with_branches_and_remotes(&mut app, vec![sample_branch("main", true)], vec![remote]);

        assert_eq!(app.focus(), Panel::Sidebar);
        assert_eq!(
            app.forge_link_target().as_deref(),
            Some("https://github.com/org/repo/tree/main")
        );
    }

    #[test]
    fn cycling_the_reference_view_only_applies_when_the_references_panel_is_focused() {
        let (mut app, _port) = new_app();
        assert_eq!(app.focus(), Panel::Sidebar);

        app.update(Action::CycleReferenceView);
        assert_eq!(
            app.reference_view(),
            ReferenceView::Tags,
            "cycling must be a no-op while a different panel is focused"
        );

        for _ in 0..4 {
            app.update(Action::FocusNext);
        }
        assert_eq!(app.focus(), Panel::References);

        app.update(Action::CycleReferenceView);
        assert_eq!(app.reference_view(), ReferenceView::Remotes);
        app.update(Action::CycleReferenceView);
        assert_eq!(app.reference_view(), ReferenceView::Stash);
        app.update(Action::CycleReferenceView);
        assert_eq!(app.reference_view(), ReferenceView::Reflog);
        app.update(Action::CycleReferenceView);
        assert_eq!(app.reference_view(), ReferenceView::Tags);
    }

    #[test]
    fn activating_a_reference_entry_opens_its_details_overlay() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_tags_loaded(generation, Ok(vec![sample_tag("v1.0")]));

        for _ in 0..4 {
            app.update(Action::FocusNext);
        }
        assert_eq!(app.focus(), Panel::References);

        app.update(Action::Activate);
        assert!(app.reference_details_open());

        app.update(Action::Dismiss);
        assert!(!app.reference_details_open());
    }

    #[test]
    fn activating_an_empty_reference_list_never_opens_a_details_overlay() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_tags_loaded(generation, Ok(vec![]));

        for _ in 0..4 {
            app.update(Action::FocusNext);
        }
        app.update(Action::Activate);
        assert!(!app.reference_details_open());
    }

    // -----------------------------------------------------------------
    // EPIC-16/T-231..T-233: merge, conflicts, continue/abort.
    // -----------------------------------------------------------------

    fn sample_merge_operation(
        conflicted: Vec<gitsail_domain::ConflictedFile>,
    ) -> InProgressOperation {
        InProgressOperation::Merge(gitsail_domain::MergeOperation {
            heads: vec![CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap()],
            conflicted_files: conflicted,
            capabilities: vec![OperationCapability::Continue, OperationCapability::Abort],
        })
    }

    fn sample_conflicted_file(path: &str) -> gitsail_domain::ConflictedFile {
        gitsail_domain::ConflictedFile {
            path: PathBuf::from(path),
            stage: gitsail_domain::ConflictStage::BothModified,
        }
    }

    #[test]
    fn requesting_merge_from_the_sidebar_begins_confirmation_naming_the_selected_branch() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true), sample_branch("develop", false)],
            vec![],
        );
        app.update(Action::MoveDown); // highlight "develop"

        app.update(Action::RequestMerge);

        match app.operation() {
            OperationState::Confirming(OperationKind::Merge { target }) => {
                assert_eq!(target, "develop");
            }
            other => panic!("expected Confirming(Merge), got {other:?}"),
        }
    }

    #[test]
    fn confirming_a_merge_dispatches_the_merge_command_with_the_exact_target() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true), sample_branch("develop", false)],
            vec![],
        );
        app.update(Action::MoveDown);
        app.update(Action::RequestMerge);

        let commands = app.update(Action::Activate);
        match commands.as_slice() {
            [Command::Merge(_, target)] => assert_eq!(target, "develop"),
            other => panic!("expected exactly one Merge command, got {other:?}"),
        }
    }

    #[test]
    fn a_successful_fast_forward_merge_records_its_outcome_and_refreshes() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true), sample_branch("develop", false)],
            vec![],
        );
        app.update(Action::MoveDown);
        app.update(Action::RequestMerge);
        app.update(Action::Activate);

        let new_head = CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap();
        let commands = app.on_merge_finished(Ok(MergeResult::FastForwarded {
            new_head: new_head.clone(),
        }));

        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::RefreshStatus(_, _))),
            "a successful merge must refresh"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::LoadInProgressOperation(_, _))),
            "a successful merge must reinspect in-progress-operation state"
        );
        assert_eq!(
            app.last_merge_result(),
            Some(&MergeResult::FastForwarded { new_head })
        );
        // A success closes its own overlay (T-267): the operation returns
        // to `Idle` and the completed kind is what the status bar reports.
        assert!(app.operation().is_idle());
        assert!(app.last_operation_outcome().is_some());
    }

    /// A conflict is a legitimate `Ok` outcome (US-079 criterion 2/3), never
    /// collapsed into `on_merge_finished`'s error path — but it must still
    /// be told apart from a plain merge by whatever renders
    /// [`App::last_merge_result`], never presented as an unqualified
    /// "success".
    #[test]
    fn a_conflicting_merge_is_reported_as_a_distinct_outcome_never_a_generic_failure() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true), sample_branch("develop", false)],
            vec![],
        );
        app.update(Action::MoveDown);
        app.update(Action::RequestMerge);
        app.update(Action::Activate);

        let files = vec![sample_conflicted_file("f.txt")];
        let commands = app.on_merge_finished(Ok(MergeResult::Conflict {
            files: files.clone(),
        }));

        assert!(!commands.is_empty(), "a conflict result must still refresh");
        match app.last_merge_result() {
            Some(MergeResult::Conflict { files: got }) => assert_eq!(got, &files),
            other => panic!("expected Some(Conflict), got {other:?}"),
        }
    }

    #[test]
    fn a_refused_merge_never_refreshes_or_records_an_outcome() {
        let (mut app, _port) = new_app();
        open_with_branches_and_remotes(
            &mut app,
            vec![sample_branch("main", true), sample_branch("develop", false)],
            vec![],
        );
        app.update(Action::MoveDown);
        app.update(Action::RequestMerge);
        app.update(Action::Activate);

        let commands = app.on_merge_finished(Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "a rebase is already in progress",
        )));

        assert!(commands.is_empty(), "a refused merge must never refresh");
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
        assert!(app.last_merge_result().is_none());
    }

    #[test]
    fn in_progress_operation_loaded_updates_state_and_clamps_a_shrunk_conflict_cursor() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();

        let files = vec![
            sample_conflicted_file("a.txt"),
            sample_conflicted_file("b.txt"),
        ];
        app.on_in_progress_operation_loaded(generation, Ok(sample_merge_operation(files)));
        app.toggle_conflicts_panel();
        app.move_cursor(1); // cursor at index 1 ("b.txt")
        assert_eq!(app.conflict_cursor(), 1);

        // A fresh load reports only one conflicted file left (the other was
        // resolved) — the cursor must never keep pointing past the end.
        let shrunk = vec![sample_conflicted_file("a.txt")];
        app.on_in_progress_operation_loaded(generation, Ok(sample_merge_operation(shrunk)));
        assert_eq!(app.conflict_cursor(), 0);

        // Once nothing is pending, the overlay closes itself rather than
        // linger open over an empty list.
        app.on_in_progress_operation_loaded(generation, Ok(InProgressOperation::None));
        assert!(!app.conflicts_open());
    }

    #[test]
    fn toggle_conflicts_panel_is_a_no_op_without_any_conflicted_files() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));

        app.toggle_conflicts_panel();

        assert!(
            !app.conflicts_open(),
            "opening the overlay with nothing conflicted must be a no-op"
        );
    }

    #[test]
    fn toggle_conflicts_panel_opens_when_conflicts_exist_and_closes_on_dismiss() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(
            generation,
            Ok(sample_merge_operation(vec![sample_conflicted_file(
                "a.txt",
            )])),
        );

        app.toggle_conflicts_panel();
        assert!(app.conflicts_open());

        app.update(Action::Dismiss);
        assert!(!app.conflicts_open());
    }

    #[test]
    fn requesting_mark_conflict_resolved_dispatches_for_the_file_under_the_cursor() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(
            generation,
            Ok(sample_merge_operation(vec![
                sample_conflicted_file("a.txt"),
                sample_conflicted_file("b.txt"),
            ])),
        );
        app.toggle_conflicts_panel();
        app.move_cursor(1);

        let commands = app.update(Action::MarkConflictResolved);

        match commands.as_slice() {
            [Command::MarkConflictResolved(_, path)] => assert_eq!(path, &PathBuf::from("b.txt")),
            other => panic!("expected exactly one MarkConflictResolved command, got {other:?}"),
        }
    }

    #[test]
    fn requesting_take_conflict_side_dispatches_the_exact_side_for_the_cursor() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(
            generation,
            Ok(sample_merge_operation(vec![sample_conflicted_file(
                "img.bin",
            )])),
        );
        app.toggle_conflicts_panel();

        let commands = app.update(Action::TakeConflictSideTheirs);

        match commands.as_slice() {
            [Command::TakeConflictSide(_, path, side)] => {
                assert_eq!(path, &PathBuf::from("img.bin"));
                assert_eq!(*side, ConflictSide::Theirs);
            }
            other => panic!("expected exactly one TakeConflictSide command, got {other:?}"),
        }
    }

    #[test]
    fn conflict_resolution_success_refreshes_and_clears_the_inspected_sides() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(
            generation,
            Ok(sample_merge_operation(vec![sample_conflicted_file(
                "a.txt",
            )])),
        );
        app.on_conflict_sides_loaded(
            PathBuf::from("a.txt"),
            Ok(ConflictSides {
                path: PathBuf::from("a.txt"),
                base: gitsail_domain::ConflictSideContent::Text("base".into()),
                ours: gitsail_domain::ConflictSideContent::Text("ours".into()),
                theirs: gitsail_domain::ConflictSideContent::Text("theirs".into()),
            }),
        );
        assert!(app.inspected_conflict().is_some());

        let commands = app.on_conflict_resolution_finished(Ok(()));

        assert!(app.inspected_conflict().is_none());
        assert!(commands
            .iter()
            .any(|c| matches!(c, Command::LoadInProgressOperation(_, _))));
    }

    #[test]
    fn conflict_resolution_failure_is_shown_inline_without_refreshing() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));

        let commands = app.on_conflict_resolution_finished(Err(GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "path is not currently conflicted",
        )));

        assert!(commands.is_empty());
        assert!(app.conflict_error().is_some());
    }

    #[test]
    fn request_continue_operation_refuses_when_the_detected_operation_does_not_support_it() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        // A bisect run supports Skip/Abort but never Continue (mirrors
        // `gitsail-git`'s own real detection).
        app.on_in_progress_operation_loaded(
            generation,
            Ok(InProgressOperation::BisectRun(
                gitsail_domain::BisectOperation {
                    conflicted_files: vec![],
                    capabilities: vec![OperationCapability::Skip, OperationCapability::Abort],
                },
            )),
        );

        app.request_continue_operation();

        assert!(
            app.operation().is_idle(),
            "continue must never be offered when the detected operation does not support it"
        );
    }

    #[test]
    fn request_continue_and_abort_begin_confirmation_when_supported() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(generation, Ok(sample_merge_operation(vec![])));

        app.request_continue_operation();
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::ContinueOperation)
        ));
        let commands = app.update(Action::Activate);
        assert!(matches!(
            commands.as_slice(),
            [Command::ContinueOperation(_)]
        ));

        app.operation.cancel();
        app.request_abort_operation();
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::AbortOperation)
        ));
        let commands = app.update(Action::Activate);
        assert!(matches!(commands.as_slice(), [Command::AbortOperation(_)]));
    }

    #[test]
    fn operation_resolution_never_presumes_success_the_next_load_is_what_tells_the_truth() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(generation, Ok(sample_merge_operation(vec![])));
        app.request_continue_operation();
        app.update(Action::Activate);

        let commands = app.on_operation_resolution_finished(Ok(()));
        assert!(commands
            .iter()
            .any(|c| matches!(c, Command::LoadInProgressOperation(_, _))));
        // Still whatever was last loaded — `on_operation_resolution_finished`
        // itself never assumes the merge is gone.
        assert!(matches!(
            app.in_progress_operation(),
            InProgressOperation::Merge(_)
        ));

        // The real state, once reinspected, is what actually clears it —
        // against the *new* generation `refresh_commands_for` (triggered by
        // `on_operation_resolution_finished` above) bumped to, exactly like
        // a stale result for an older generation must be discarded
        // elsewhere in this module.
        let latest_generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(latest_generation, Ok(InProgressOperation::None));
        assert!(app.in_progress_operation().is_none());
    }

    #[test]
    fn a_refused_continue_or_abort_fails_without_refreshing() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_in_progress_operation_loaded(generation, Ok(sample_merge_operation(vec![])));
        app.request_continue_operation();
        app.update(Action::Activate);

        let commands = app.on_operation_resolution_finished(Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "unresolved conflicted files remain",
        )));

        assert!(commands.is_empty());
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
    }

    // -----------------------------------------------------------------
    // T-241/US-089: inspect HEAD's reflog.
    // -----------------------------------------------------------------

    fn sample_reflog_entry(index: u32, state: gitsail_domain::ReflogObjectState) -> ReflogEntry {
        ReflogEntry {
            index,
            commit: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            message: format!("commit: entry {index}"),
            date: gitsail_domain::GitTimestamp::new(1_000, 0),
            object_state: state,
        }
    }

    fn sample_commit_for_details() -> gitsail_domain::Commit {
        gitsail_domain::Commit {
            hash: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            short_hash: gitsail_domain::ShortHash::new("deadbee".to_string()).unwrap(),
            parents: vec![],
            author: gitsail_domain::Signature::new("Ada Lovelace", "ada@example.com"),
            committer: gitsail_domain::Signature::new("Ada Lovelace", "ada@example.com"),
            author_date: gitsail_domain::GitTimestamp::new(1_000, 0),
            commit_date: gitsail_domain::GitTimestamp::new(1_000, 0),
            subject: "a sample commit".to_string(),
            body: String::new(),
            decorations: vec![],
        }
    }

    // -- T-251/US-109 criterion 3: confirmation cannot be skipped ----------

    /// Opens a repository with one branch (`main`, current) and one commit
    /// loaded into the Graph panel, focused there — the minimal fixture
    /// [`Action::RequestReset`] needs (a selected commit) plus
    /// [`App::current_head_hash`] needs (a `Local` branch matching the
    /// attached `HEAD`'s name).
    fn open_repository_with_one_graph_commit(app: &mut App) {
        let open_commands = app.on_repository_opened(Ok(sample_repository()));
        let ticket = match open_commands.first() {
            Some(Command::RefreshStatus(t, _)) => *t,
            other => panic!("unexpected first command: {other:?}"),
        };
        app.on_status_refreshed(ticket, Ok(clean_status()));

        let generation = app.session().unwrap().generation();
        app.on_branches_loaded(generation, Ok(vec![sample_branch("main", true)]));

        // `restart_commit_graph` (called from `on_repository_opened`) always
        // bumps `graph_request_id` to `1` on a session's very first open.
        app.on_commit_graph_page_loaded(
            1,
            Ok(Page {
                items: vec![sample_commit_for_details()],
                next_cursor: None,
                has_more: false,
            }),
        );

        app.update(Action::FocusNext); // Sidebar -> Graph
        assert_eq!(app.focus(), Panel::Graph);
        assert!(app.selected_graph_commit().is_some());
    }

    /// The load-bearing proof for T-251/US-109 criterion 3 ("a política de
    /// confirmação não pode ser desabilitada por preferência"): opening a
    /// `Destructive`-risk operation (`Hard` reset) and running it to
    /// completion always takes **two** distinct [`Action::Activate`]
    /// dispatches — the first only picks the mode and starts confirmation
    /// ([`OperationState::Confirming`]), never dispatching a [`Command`];
    /// the second is what actually confirms and dispatches the mutation.
    /// This is true independent of the TUI's own keybindings-remapping
    /// mechanism (`crate::keybindings`) added by this same story: that
    /// mechanism only ever resolves *which key* produces an [`Action`] in
    /// [`crate::keymap::InputContext::Normal`] (see its own module doc);
    /// it is never consulted for `Enter`/`Esc` in any of the overlay
    /// contexts this flow passes through
    /// (`crate::keymap::InputContext::ResetMode`, then `Normal` again
    /// while `operation` is `Confirming` — see `App::input_context`'s own
    /// doc for why), so there is no remapping, however constructed, that
    /// could ever collapse this into a single keypress.
    #[test]
    fn a_destructive_reset_still_requires_two_explicit_confirmations() {
        let (mut app, _port) = new_app();
        open_repository_with_one_graph_commit(&mut app);

        // Opening the chooser is itself not a mutation, and starts no
        // operation at all yet.
        let commands = app.update(Action::RequestReset);
        assert!(commands.is_empty());
        assert!(app.reset_mode_open());
        assert!(app.operation().is_idle());

        // Move the chooser's cursor to `Hard` (index 2 of [Soft, Mixed,
        // Hard]).
        app.update(Action::MoveDown);
        app.update(Action::MoveDown);
        assert_eq!(app.reset_mode_cursor(), 2);

        // First `Activate`: picks the mode, starts confirmation. Still no
        // `Command` dispatched — nothing has mutated yet.
        let commands_after_first_activate = app.update(Action::Activate);
        assert!(
            commands_after_first_activate.is_empty(),
            "picking a reset mode must never itself dispatch a mutation"
        );
        assert!(
            !app.reset_mode_open(),
            "the chooser hands off to the generic confirmation prompt"
        );
        match app.operation() {
            OperationState::Confirming(OperationKind::Reset {
                mode: ResetMode::Hard,
                ..
            }) => {}
            other => panic!("expected Confirming(Reset {{ Hard }}), got {other:?}"),
        }

        // Second `Activate`: only *now* does the mutation actually
        // dispatch.
        let commands_after_second_activate = app.update(Action::Activate);
        assert!(
            matches!(commands_after_second_activate.as_slice(), [Command::Reset(..)]),
            "the second confirmation must be what actually dispatches the reset, got {commands_after_second_activate:?}"
        );
        assert!(matches!(app.operation(), OperationState::InProgress(_)));
    }

    /// The same guarantee from the opposite direction: cancelling at the
    /// first confirmation prompt (`Esc`/[`Action::Dismiss`]) — reachable
    /// exactly like any other overlay — leaves the repository untouched,
    /// exactly like every other `Confirming` state already does (History
    /// Editing Rules / Destructive Operations Guardrails wiki rule 5:
    /// "Cancelling before confirmation leaves the repository completely
    /// untouched").
    #[test]
    fn dismissing_a_pending_destructive_reset_confirmation_dispatches_nothing() {
        let (mut app, _port) = new_app();
        open_repository_with_one_graph_commit(&mut app);

        app.update(Action::RequestReset);
        app.update(Action::MoveDown);
        app.update(Action::MoveDown);
        app.update(Action::Activate);
        assert!(matches!(
            app.operation(),
            OperationState::Confirming(OperationKind::Reset {
                mode: ResetMode::Hard,
                ..
            })
        ));

        app.update(Action::Dismiss);

        assert!(
            app.operation().is_idle(),
            "dismissing a pending confirmation must cancel it, never silently confirm it"
        );
    }

    /// Focuses the References panel and cycles its sub-view to Reflog
    /// (Tags -> Remotes -> Stash -> Reflog).
    fn open_references_on_reflog(app: &mut App) {
        for _ in 0..4 {
            app.update(Action::FocusNext);
        }
        assert_eq!(app.focus(), Panel::References);
        for _ in 0..3 {
            app.update(Action::CycleReferenceView);
        }
        assert_eq!(app.reference_view(), ReferenceView::Reflog);
    }

    #[test]
    fn reflog_entries_are_listed_after_loading_newest_first() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();

        app.on_reflog_loaded(
            generation,
            Ok(vec![
                sample_reflog_entry(0, gitsail_domain::ReflogObjectState::Present),
                sample_reflog_entry(1, gitsail_domain::ReflogObjectState::Missing),
            ]),
        );

        assert_eq!(app.reflog().len(), 2);
        assert_eq!(app.reflog()[0].selector("HEAD"), "HEAD@{0}");
        assert!(app.reflog()[0].is_available());
        assert!(
            !app.reflog()[1].is_available(),
            "US-089 criterion 3: an expired/pruned entry must be reported, not hidden"
        );
    }

    #[test]
    fn a_stale_reflog_result_is_discarded() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let stale_generation = app.session().unwrap().generation();

        app.update(Action::Refresh);
        app.on_reflog_loaded(
            stale_generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Present,
            )]),
        );

        assert!(
            app.reflog().is_empty(),
            "a reflog result computed for an old generation must not populate the panel"
        );
    }

    /// US-089 criterion 2: selecting an entry whose commit still exists
    /// opens its details — reusing `GetCommit` (via
    /// `Command::LoadReflogCommit`), never a parallel read.
    #[test]
    fn activating_an_available_reflog_entry_dispatches_a_commit_load_and_opens_the_overlay() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_reflog_loaded(
            generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Present,
            )]),
        );
        open_references_on_reflog(&mut app);

        let commands = app.update(Action::Activate);

        assert!(app.reflog_details_open());
        assert!(app.reflog_details_error().is_none());
        assert!(app.reflog_details_commit().is_none(), "still loading");
        match commands.as_slice() {
            [Command::LoadReflogCommit(_, hash)] => {
                assert_eq!(hash.as_str(), "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
            }
            other => panic!("expected exactly one LoadReflogCommit command, got {other:?}"),
        }
    }

    /// US-089 criterion 3: an entry whose object no longer exists states
    /// that clearly, and never even attempts a read for it — this is never a
    /// silent omission, and the inspection never runs a reset (History
    /// Editing Rules #10).
    #[test]
    fn activating_a_missing_reflog_entry_states_that_clearly_without_reading_anything() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_reflog_loaded(
            generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Missing,
            )]),
        );
        open_references_on_reflog(&mut app);

        let commands = app.update(Action::Activate);

        assert!(app.reflog_details_open());
        assert!(
            commands.is_empty(),
            "an expired/pruned entry's object must never be read"
        );
        assert!(app.reflog_details_error().is_some());
        assert!(app.reflog_details_commit().is_none());
    }

    #[test]
    fn on_reflog_commit_loaded_discards_a_result_for_an_entry_no_longer_under_the_cursor() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_reflog_loaded(
            generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Present,
            )]),
        );
        open_references_on_reflog(&mut app);
        app.update(Action::Activate);

        let abandoned_hash = CommitHash::new("cafecafecafecafecafecafecafecafecafecafe").unwrap();
        app.on_reflog_commit_loaded(abandoned_hash, Ok(sample_commit_for_details()));

        assert!(
            app.reflog_details_commit().is_none(),
            "a result for a hash no longer under the cursor must be discarded"
        );
    }

    #[test]
    fn on_reflog_commit_loaded_populates_the_overlay_for_the_current_entry() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_reflog_loaded(
            generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Present,
            )]),
        );
        open_references_on_reflog(&mut app);
        app.update(Action::Activate);

        let hash = CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        app.on_reflog_commit_loaded(hash, Ok(sample_commit_for_details()));

        assert_eq!(
            app.reflog_details_commit().unwrap().subject,
            "a sample commit"
        );
        assert!(app.reflog_details_error().is_none());
    }

    #[test]
    fn dismissing_reflog_details_closes_it_without_touching_the_operation_state() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        let generation = app.session().unwrap().generation();
        app.on_reflog_loaded(
            generation,
            Ok(vec![sample_reflog_entry(
                0,
                gitsail_domain::ReflogObjectState::Present,
            )]),
        );
        open_references_on_reflog(&mut app);
        app.update(Action::Activate);
        assert!(app.reflog_details_open());

        app.update(Action::Dismiss);

        assert!(!app.reflog_details_open());
        assert!(app.reflog_details_commit().is_none());
        assert!(app.operation().is_idle());
    }

    // -----------------------------------------------------------------
    // T-242/US-090: amend the last commit with the TUI.
    // -----------------------------------------------------------------

    fn sample_amend_preview(subject: &str, body: &str) -> AmendPreview {
        AmendPreview {
            head: gitsail_domain::Commit {
                subject: subject.to_string(),
                body: body.to_string(),
                ..sample_commit_for_details()
            },
            staged_diff: Diff { files: vec![] },
        }
    }

    #[test]
    fn starting_amend_dispatches_a_preview_read_and_is_idempotent_while_open() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));

        let commands = app.update(Action::StartAmend);
        assert!(app.amend_open());
        assert!(matches!(commands.as_slice(), [Command::PreviewAmend(_)]));

        // A second `A` press while already open must never discard an
        // in-flight/edited state by restarting the preview.
        let commands = app.update(Action::StartAmend);
        assert!(commands.is_empty());
    }

    /// US-090 criterion 1: the TUI reuses `gitsail_application::PreviewAmend`
    /// unchanged — the message is pre-filled from HEAD's own subject/body,
    /// mirroring `apps/desktop/src/stores/amend.ts`'s own prefill exactly.
    #[test]
    fn amend_preview_prefills_the_message_from_heads_subject_and_body() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);

        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "details here")));

        assert_eq!(app.amend_message(), Some("fix bug\n\ndetails here"));
        assert!(app.amend_error().is_none());
        assert!(app.amend_preview().is_some());
    }

    #[test]
    fn amend_preview_with_no_body_prefills_only_the_subject() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);

        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "")));

        assert_eq!(app.amend_message(), Some("fix bug"));
    }

    #[test]
    fn a_failed_amend_preview_is_shown_inline_without_opening_a_composer_message() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);

        app.on_amend_previewed(Err(GitSailError::new(
            ErrorCode::RepositoryNotFound,
            "HEAD could not be resolved",
        )));

        assert!(app.amend_error().is_some());
        assert!(app.amend_message().is_none());
    }

    /// US-090 criteria 1, 2: confirming amend names the exact commit being
    /// replaced and classifies `Destructive` — reusing
    /// `gitsail_application::AmendCommit`'s existing `expected_head`
    /// revalidation contract (never a parallel implementation), and the
    /// same risk level `apps/desktop/src/stores/amend.ts` already uses.
    #[test]
    fn confirming_amend_begins_a_destructive_confirmation_naming_head() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);
        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "")));

        app.update(Action::Activate);

        match app.operation() {
            OperationState::Confirming(kind) => {
                assert_eq!(kind.risk(), crate::operation::OperationRisk::Destructive);
                let label = kind.prompt_label();
                assert!(label.contains("deadbee"));
                assert!(label.contains("pushed or shared"));
            }
            other => panic!("expected Confirming(AmendCommit), got {other:?}"),
        }
    }

    #[test]
    fn confirming_amend_without_a_loaded_preview_is_a_no_op() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);

        let commands = app.update(Action::Activate);

        assert!(commands.is_empty());
        assert!(app.operation().is_idle());
    }

    #[test]
    fn dispatching_a_confirmed_amend_sends_the_edited_message_and_the_expected_head() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);
        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "")));
        for c in " — edited".chars() {
            app.update(Action::AmendMessageInput(c));
        }
        app.update(Action::Activate); // -> Confirming

        let commands = app.update(Action::Activate); // -> confirm, dispatch

        match commands.as_slice() {
            [Command::AmendCommit(_, message, expected_head)] => {
                assert_eq!(message, "fix bug — edited");
                assert_eq!(
                    expected_head.as_str(),
                    "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
                );
            }
            other => panic!("expected exactly one AmendCommit command, got {other:?}"),
        }
        assert!(matches!(app.operation(), OperationState::InProgress(_)));
    }

    /// US-090 criterion 3 / DoD: mirrors the Desktop's own stale-`HEAD` race
    /// test — a rejected amend (Core's `amend_commit` revalidates
    /// `expected_head` immediately before running, and refuses when `HEAD`
    /// moved since the preview) must never be silently treated as a success,
    /// and must never lose the typed message or discard staged changes.
    /// `amend_commit` itself only ever touches the index/working tree on an
    /// actual success, so "staged alterations preserved" holds by
    /// construction whenever this failure path is taken.
    #[test]
    fn a_failed_amend_from_a_stale_head_preserves_the_message_and_never_refreshes() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);
        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "")));
        app.update(Action::Activate);
        app.update(Action::Activate);

        let commands = app.on_amend_finished(Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "HEAD changed since the amend was previewed",
        )));

        assert!(commands.is_empty(), "a rejected amend must never refresh");
        assert!(matches!(app.operation(), OperationState::Failed(_, _)));
        assert_eq!(
            app.amend_message(),
            Some("fix bug"),
            "the typed message must survive a rejected amend"
        );
        assert!(
            app.amend_open(),
            "the composer must still be showing the preserved message"
        );
    }

    #[test]
    fn a_successful_amend_clears_the_composer_and_refreshes_status_and_the_commit_graph() {
        let (mut app, _port) = new_app();
        app.on_repository_opened(Ok(sample_repository()));
        app.update(Action::StartAmend);
        app.on_amend_previewed(Ok(sample_amend_preview("fix bug", "")));
        app.update(Action::Activate);
        app.update(Action::Activate);

        let commands = app.on_amend_finished(Ok(CommitHash::new(
            "cafef00dcafef00dcafef00dcafef00dcafef00d",
        )
        .unwrap()));

        assert!(!app.amend_open());
        assert!(app.amend_message().is_none());
        assert!(app.amend_preview().is_none());
        // A success closes its own overlay (T-267): the operation returns
        // to `Idle` and the completed kind is what the status bar reports.
        assert!(app.operation().is_idle());
        assert!(app.last_operation_outcome().is_some());
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::RefreshStatus(_, _))),
            "success must refresh status"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Command::LoadCommitGraph(_, _, _))),
            "success must refresh the commit graph, since amend rewrites HEAD's own tip"
        );
    }
}
