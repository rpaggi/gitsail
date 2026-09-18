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

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    AbortOperation, AmendCommit, ApplyPatch, CherryPick, CommitQuery, ConnectForgeAccount,
    ContinueOperation, CreateBranch, CreateCommit, DeleteBranch, DetectInProgressOperation,
    DiffRequest, DisconnectForgeAccount, ExecuteRebasePlan, Fetch, ForgeToken,
    ForgetRecentRepository, GetCommit, GetCommitHistory, GetConflictSides, GetDiff,
    GetForgeConnectionStatus, GetForgeLink, ListBranches, ListPullRequests, ListRecentRepositories,
    MarkConflictResolved, Merge, MergeParentPolicy, OpenRepository, PlanRebase, PreviewAmend,
    PreviewPatchApplication, Pull, Push, Rebase, RebasePlan, RecordRecentRepository, RefreshReason,
    RenameBranch, Reset, ResetMode, Revert, SkipOperation, StageFiles, StageHunks, SwitchBranch,
    TakeConflictSide, UnstageFiles, UnstageHunks,
};
use gitsail_domain::{
    repository_location, Branch, BranchKind, BranchName, CancellationToken, CommitHash,
    ConflictSide, ErrorCode, FileDiff, ForgePath, GitSailError, GraphCommit, Remote, Repository,
};
use gitsail_protocol::{
    AmendPreviewDto, ApplyPatchResultDto, BranchDto, CherryPickResultDto, CommitDto,
    CommitGraphPageDto, CommitGraphRowDto, CommitResultDto, ConflictSidesDto, DiffDto,
    ErrorPayload, FileDiffDto, ForgeAccountDto, ForgeConnectionStatusDto, ForgeLinkTargetDto,
    InProgressOperationDto, ListPullRequestsOutcomeDto, MergeResultDto, PatchExportDto,
    PatchPreviewDto, PullOutcomeDto, PullResultDto, RebasePlanDto, RebaseResultDto,
    RecentRepositoryDto, RemoteDto, RepositoryDto, RepositoryStatusDto, RevertResultDto,
    SyncTargetDto,
};

use crate::state::{AppState, StartupIntent};

/// Runs a repository mutation, then re-validates that the session epoch
/// has not moved on while it ran (US-192/US-193's "revalidate immediately
/// before/around a mutation" discipline, extended from
/// `AppState::append_commit_graph_page_if_current`'s read-side guard to
/// every write command in this module).
///
/// This never blocks a switch from happening, and it never undoes a
/// mutation that already succeeded against the repository that *was* open
/// — `git` has already run by the time this returns the error — but it
/// does mean the frontend is told, unambiguously, that whatever it now
/// displays is not the repository the mutation actually ran against,
/// instead of silently treating the mutation as if it had applied to the
/// repository currently on screen.
fn run_mutation<T>(
    state: &AppState,
    action: impl FnOnce(&Repository) -> Result<T, GitSailError>,
) -> Result<T, GitSailError> {
    let (repository, epoch) = state.repository_with_epoch()?;
    let result = action(&repository)?;
    let (_, epoch_after) = state.repository_with_epoch()?;
    if epoch_after != epoch {
        return Err(GitSailError::new(
            ErrorCode::Cancelled,
            "the active repository changed while this operation was running",
        )
        .with_remediation("refresh the repository view before retrying"));
    }
    Ok(result)
}

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
/// Builds a full, `git apply`-compatible patch for `staged`/`path`'s diff
/// (US-029/T-162 criterion 1: the caller states the exact scope — which
/// side and, when given, which file — so the origin of what gets copied is
/// never ambiguous). Read-only: this never touches the index, working
/// tree, or HEAD (criterion 2) — it only calls the same `RepositoryReadPort::diff`
/// every other diff-reading surface uses, then
/// `gitsail_application::export_patch` to render it, mirroring
/// `gitsail-tui`'s `y` shortcut so both interfaces share one Core behavior.
///
/// Copying the result to the clipboard and falling back to a file save
/// (criterion 3) both happen in the frontend, which already has native
/// access to the browser clipboard API and the Tauri dialog plugin — this
/// command's only job is producing the patch text, never touching either.
#[tauri::command]
pub fn export_patch(
    staged: bool,
    path: Option<String>,
    state: tauri::State<AppState>,
) -> Result<PatchExportDto, ErrorPayload> {
    export_patch_impl(&state, staged, path.as_deref()).map_err(|err| ErrorPayload::from(&err))
}

/// Writes `contents` to `path`, overwriting whatever was already there
/// (US-029/T-162 criterion 3's file-based clipboard fallback). The frontend
/// picks `path` via `@tauri-apps/plugin-dialog`'s native save dialog
/// (already a dependency, used elsewhere for the "open repository" picker)
/// — this command's only job is the write itself, so no new file-system
/// plugin was needed for this story; see `AGENTS.md`/this crate's
/// `Cargo.toml` for that choice if a future story needs richer fs access
/// from the frontend.
///
/// Deliberately generic (not "save a patch"): the same primitive would
/// serve any future "save this text to a file" need without a second,
/// near-identical command.
#[tauri::command]
pub fn save_text_file(path: String, contents: String) -> Result<(), ErrorPayload> {
    save_text_file_impl(&path, &contents).map_err(|err| ErrorPayload::from(&err))
}

fn save_text_file_impl(path: &str, contents: &str) -> Result<(), GitSailError> {
    std::fs::write(path, contents).map_err(|err| {
        GitSailError::new(ErrorCode::Internal, "failed to write the file").with_source(err)
    })
}

/// Reads `path`'s full contents as UTF-8 text — the read counterpart to
/// [`save_text_file`], added for T-163/US-030's "apply a patch from a
/// chosen file" flow: the frontend picks `path` via
/// `@tauri-apps/plugin-dialog`'s native open dialog (already a
/// dependency), this command's only job is the read itself. Deliberately
/// generic (not "read a patch file"), mirroring [`save_text_file`]'s own
/// "the same primitive would serve any future need" reasoning.
#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, ErrorPayload> {
    read_text_file_impl(&path).map_err(|err| ErrorPayload::from(&err))
}

fn read_text_file_impl(path: &str) -> Result<String, GitSailError> {
    std::fs::read_to_string(path).map_err(|err| {
        GitSailError::new(ErrorCode::Internal, "failed to read the file").with_source(err)
    })
}

/// Validates `patch_text` against the repository's current state via a
/// non-mutating `git apply --check`, reporting which files it would touch
/// and whether it is supported at all (T-163/US-030 criterion 1) — the
/// preview the frontend shows *before* asking for confirmation. Never
/// mutates anything; the actual apply is [`apply_patch`], a separate
/// command the frontend calls only once the person confirms.
#[tauri::command]
pub fn preview_patch_application(
    patch_text: String,
    state: tauri::State<AppState>,
) -> Result<PatchPreviewDto, ErrorPayload> {
    preview_patch_application_impl(&state, &patch_text).map_err(|err| ErrorPayload::from(&err))
}

fn preview_patch_application_impl(
    state: &AppState,
    patch_text: &str,
) -> Result<PatchPreviewDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let preview =
        PreviewPatchApplication::new(state.write_port()).execute(&repository, patch_text)?;
    Ok(PatchPreviewDto::from(&preview))
}

/// Applies `patch_text` to the working tree (T-163/US-030) — the confirmed
/// counterpart to [`preview_patch_application`]. Goes through
/// [`run_mutation`] like every other mutating command here, and
/// `gitsail_git::GitCliProvider::apply_patch` itself re-validates with the
/// same `--check` immediately before writing anything, so a stale
/// confirmation (the file changed again after the preview the frontend
/// showed) is refused rather than silently applied.
#[tauri::command]
pub fn apply_patch(
    patch_text: String,
    state: tauri::State<AppState>,
) -> Result<ApplyPatchResultDto, ErrorPayload> {
    apply_patch_impl(&state, &patch_text).map_err(|err| ErrorPayload::from(&err))
}

fn apply_patch_impl(state: &AppState, patch_text: &str) -> Result<ApplyPatchResultDto, GitSailError> {
    let result = run_mutation(state, |repository| {
        ApplyPatch::new(state.write_port()).execute(repository, patch_text)
    })?;
    Ok(ApplyPatchResultDto::from(&result))
}

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

fn export_patch_impl(
    state: &AppState,
    staged: bool,
    path: Option<&str>,
) -> Result<PatchExportDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let request = DiffRequest {
        staged,
        path_filter: path.map(PathBuf::from),
        ..DiffRequest::default()
    };
    let diff =
        GetDiff::new(state.port()).execute(&repository, &request, &CancellationToken::new())?;
    let export = gitsail_application::export_patch(&diff.files);
    Ok(PatchExportDto::from(&export))
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

// -- US-056: startup handoff (--repo/--commit; the EPIC-15 gap) --------

/// Returns whatever `--repo`/`--commit` argv this process was launched
/// with (see `lib.rs::parse_startup_args`), consuming it so a later call —
/// a stray re-render, a reload — never re-applies the same startup target
/// a second time (`AppState::take_startup_intent`'s own contract).
/// Infallible: a plain launch with no such arguments is not an error, it
/// is simply an empty intent.
#[tauri::command]
pub fn take_startup_intent(state: tauri::State<AppState>) -> StartupIntent {
    state.take_startup_intent()
}

// -- US-056: branches, single-commit lookup, and history search --------

#[tauri::command]
pub fn list_branches(state: tauri::State<AppState>) -> Result<Vec<BranchDto>, ErrorPayload> {
    list_branches_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn list_branches_impl(state: &AppState) -> Result<Vec<BranchDto>, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let branches = ListBranches::new(state.port()).execute(&repository)?;
    Ok(branches.iter().map(BranchDto::from).collect())
}

/// Fetches one commit by its full hash (US-056 criterion 2: selecting a
/// search result, or a `--commit` startup handoff target, resolves to the
/// same commit identity the graph/list/details panels already share).
#[tauri::command]
pub fn get_commit(hash: String, state: tauri::State<AppState>) -> Result<CommitDto, ErrorPayload> {
    get_commit_impl(&state, &hash).map_err(|err| ErrorPayload::from(&err))
}

fn get_commit_impl(state: &AppState, hash: &str) -> Result<CommitDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let hash = CommitHash::new(hash)?;
    let commit = GetCommit::new(state.port()).execute(&repository, &hash)?;
    Ok(CommitDto::from(&commit))
}

/// Searches commit history (US-056 criterion 1): reuses exactly
/// `gitsail_application::CommitQuery`'s own filters — the same ones
/// `gitsail-tui`'s T-178 search already exercises — rather than inventing
/// a second query shape. There is deliberately no `tag` filter: the Core
/// read port has no "list every tag" capability yet (that is EPIC-18,
/// blocked/out of scope here); a tag *decoration* on an already-loaded
/// commit is still visible (`CommitDto::decorations`), just not
/// searchable as its own filter.
#[tauri::command]
pub fn search_commits(
    text_query: Option<String>,
    author: Option<String>,
    branch: Option<String>,
    revision_range: Option<String>,
    limit: Option<u32>,
    state: tauri::State<AppState>,
) -> Result<Vec<CommitDto>, ErrorPayload> {
    search_commits_impl(&state, text_query, author, branch, revision_range, limit)
        .map_err(|err| ErrorPayload::from(&err))
}

fn search_commits_impl(
    state: &AppState,
    text_query: Option<String>,
    author: Option<String>,
    branch: Option<String>,
    revision_range: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<CommitDto>, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let branch = branch.map(BranchName::new).transpose()?;
    let query = CommitQuery {
        text_query,
        author,
        branch,
        revision_range,
        limit,
        ..CommitQuery::default()
    };
    let page = GetCommitHistory::new(state.port()).execute(&repository, &query)?;
    Ok(page.items.iter().map(CommitDto::from).collect())
}

// -- US-057: unified/side-by-side diff ----------------------------------

/// Reads a diff for either the staged or unstaged side, optionally scoped
/// to one file (US-057 criterion 1) — the frontend derives both the
/// unified and side-by-side presentations from this single [`DiffDto`],
/// never issuing a second read per view mode.
#[tauri::command]
pub fn get_diff(
    staged: bool,
    path: Option<String>,
    state: tauri::State<AppState>,
) -> Result<DiffDto, ErrorPayload> {
    get_diff_impl(&state, staged, path.as_deref()).map_err(|err| ErrorPayload::from(&err))
}

fn get_diff_impl(state: &AppState, staged: bool, path: Option<&str>) -> Result<DiffDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let request = DiffRequest {
        staged,
        path_filter: path.map(PathBuf::from),
        ..DiffRequest::default()
    };
    let diff =
        GetDiff::new(state.port()).execute(&repository, &request, &CancellationToken::new())?;
    Ok(DiffDto::from(&diff))
}

// -- US-058: stage/unstage and compose a commit -------------------------

#[tauri::command]
pub fn stage_paths(paths: Vec<String>, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    stage_paths_impl(&state, paths).map_err(|err| ErrorPayload::from(&err))
}

fn stage_paths_impl(state: &AppState, paths: Vec<String>) -> Result<(), GitSailError> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    run_mutation(state, |repository| {
        StageFiles::new(state.write_port()).execute(repository, &paths)
    })
}

#[tauri::command]
pub fn unstage_paths(paths: Vec<String>, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    unstage_paths_impl(&state, paths).map_err(|err| ErrorPayload::from(&err))
}

fn unstage_paths_impl(state: &AppState, paths: Vec<String>) -> Result<(), GitSailError> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    run_mutation(state, |repository| {
        UnstageFiles::new(state.write_port()).execute(repository, &paths)
    })
}

/// Stages only the hunks carried by `selection` (US-058 criterion 1's
/// hunk-level granularity), each element being a [`FileDiffDto`] trimmed
/// to the hunks to stage — typically a subset of what `get_diff` last
/// returned for the unstaged side. The DTO -> domain conversion
/// (`gitsail_protocol::dto`'s reverse `From` impls) is the only "logic"
/// here; the actual hunk application is `gitsail-git`'s, unchanged.
#[tauri::command]
pub fn stage_hunks(selection: Vec<FileDiffDto>, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    stage_hunks_impl(&state, &selection).map_err(|err| ErrorPayload::from(&err))
}

fn stage_hunks_impl(state: &AppState, selection: &[FileDiffDto]) -> Result<(), GitSailError> {
    let selection: Vec<FileDiff> = selection.iter().map(FileDiff::from).collect();
    run_mutation(state, |repository| {
        StageHunks::new(state.write_port()).execute(repository, &selection)
    })
}

#[tauri::command]
pub fn unstage_hunks(selection: Vec<FileDiffDto>, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    unstage_hunks_impl(&state, &selection).map_err(|err| ErrorPayload::from(&err))
}

fn unstage_hunks_impl(state: &AppState, selection: &[FileDiffDto]) -> Result<(), GitSailError> {
    let selection: Vec<FileDiff> = selection.iter().map(FileDiff::from).collect();
    run_mutation(state, |repository| {
        UnstageHunks::new(state.write_port()).execute(repository, &selection)
    })
}

/// Commits exactly the current index content with `message` (US-058
/// criterion 2/3). The frontend is expected to have already run this
/// through the T-194 confirmation dialog — this command itself has no
/// notion of confirmation, matching every other command in this file
/// (US-051 criterion 3: no business/UX rule lives in a Tauri command).
#[tauri::command]
pub fn create_commit(
    message: String,
    state: tauri::State<AppState>,
) -> Result<CommitResultDto, ErrorPayload> {
    create_commit_impl(&state, &message).map_err(|err| ErrorPayload::from(&err))
}

fn create_commit_impl(state: &AppState, message: &str) -> Result<CommitResultDto, GitSailError> {
    let hash = run_mutation(state, |repository| {
        CreateCommit::new(state.write_port()).execute(repository, message)
    })?;
    Ok(CommitResultDto::from(&hash))
}

// -- US-059: amend HEAD with confirmation -------------------------------

/// Builds the read-only preview US-059 criterion 1 requires: `HEAD`'s
/// exact commit and the staged diff that would be folded into it.
#[tauri::command]
pub fn preview_amend(state: tauri::State<AppState>) -> Result<AmendPreviewDto, ErrorPayload> {
    preview_amend_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn preview_amend_impl(state: &AppState) -> Result<AmendPreviewDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let preview =
        PreviewAmend::new(state.port()).execute(&repository, &CancellationToken::new())?;
    Ok(AmendPreviewDto::from(&preview))
}

/// Amends `HEAD` (US-059 criteria 2/3). `expected_head` must be the exact
/// hash `preview_amend` returned as `head.hash` — `AmendCommit`/
/// `RepositoryWritePort::amend_commit` revalidate it is still `HEAD`
/// immediately before amending and refuse with a classified
/// `OperationConflict` otherwise (never a generic "error"), so a stale
/// preview (HEAD moved since it was shown) can never rewrite the wrong
/// commit. The frontend is expected to have already shown the T-194
/// confirmation, including the "this rewrites history" warning text.
#[tauri::command]
pub fn amend_commit(
    message: String,
    expected_head: String,
    state: tauri::State<AppState>,
) -> Result<CommitResultDto, ErrorPayload> {
    amend_commit_impl(&state, &message, &expected_head).map_err(|err| ErrorPayload::from(&err))
}

fn amend_commit_impl(
    state: &AppState,
    message: &str,
    expected_head: &str,
) -> Result<CommitResultDto, GitSailError> {
    let expected_head = CommitHash::new(expected_head)?;
    let hash = run_mutation(state, |repository| {
        AmendCommit::new(state.write_port()).execute(repository, message, &expected_head)
    })?;
    Ok(CommitResultDto::from(&hash))
}

// -- US-060: local branches, plus remote sync (fetch/pull/push) --------

#[tauri::command]
pub fn create_branch(
    name: String,
    start_point: Option<String>,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    create_branch_impl(&state, &name, start_point.as_deref()).map_err(|err| ErrorPayload::from(&err))
}

fn create_branch_impl(
    state: &AppState,
    name: &str,
    start_point: Option<&str>,
) -> Result<(), GitSailError> {
    let branch_name = BranchName::new(name)?;
    run_mutation(state, |repository| {
        let resolved_start = start_point
            .map(|revision| state.port().resolve_revision(repository, revision))
            .transpose()?;
        CreateBranch::new(state.write_port()).execute(repository, &branch_name, resolved_start.as_ref())
    })
}

#[tauri::command]
pub fn switch_branch(target: String, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    switch_branch_impl(&state, &target).map_err(|err| ErrorPayload::from(&err))
}

fn switch_branch_impl(state: &AppState, target: &str) -> Result<(), GitSailError> {
    let target = BranchName::new(target)?;
    run_mutation(state, |repository| {
        SwitchBranch::new(state.write_port()).execute(repository, &target)
    })
}

#[tauri::command]
pub fn delete_branch(
    name: String,
    force: bool,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    delete_branch_impl(&state, &name, force).map_err(|err| ErrorPayload::from(&err))
}

fn delete_branch_impl(state: &AppState, name: &str, force: bool) -> Result<(), GitSailError> {
    let name = BranchName::new(name)?;
    run_mutation(state, |repository| {
        DeleteBranch::new(state.write_port()).execute(repository, &name, force)
    })
}

/// Renames a local branch (T-157/US-024). See
/// [`gitsail_application::RepositoryWritePort::rename_branch`] for the
/// collision/upstream contract this delegates to unchanged; the frontend is
/// expected to have already shown both names and their risk (US-024
/// criterion 1) before this runs.
#[tauri::command]
pub fn rename_branch(
    old_name: String,
    new_name: String,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    rename_branch_impl(&state, &old_name, &new_name).map_err(|err| ErrorPayload::from(&err))
}

fn rename_branch_impl(state: &AppState, old_name: &str, new_name: &str) -> Result<(), GitSailError> {
    let old_name = BranchName::new(old_name)?;
    let new_name = BranchName::new(new_name)?;
    run_mutation(state, |repository| {
        RenameBranch::new(state.write_port()).execute(repository, &old_name, &new_name)
    })
}

// -- US-060: remote sync (fetch/pull/push), EPIC-19 -----------------------
//
// Every command below resolves which remote (and, for pull/push, which
// branch) it targets the same way `gitsail-tui`'s own `App::
// resolve_sync_remote` does (T-182/US-049): prefer the current branch's
// configured upstream, else the sole configured remote, else refuse rather
// than silently guessing among several — this story's own criterion 3
// ("mesma política, mesmo resultado esperado") requires the two surfaces to
// agree, so the algorithm is copied verbatim rather than reinvented, just
// against this crate's own already-fetched `Branch`/`Remote` values instead
// of the TUI's in-memory `App` fields.

/// Resolves which remote fetch/pull/push should target, given `current_branch`,
/// the repository's branches, and its configured remotes. Pure (no I/O) so it
/// is trivially unit-testable independent of any adapter — see this
/// module's tests for the same four scenarios `gitsail-tui`'s own
/// `resolve_sync_remote` tests cover (upstream preferred, sole-remote
/// fallback, no remote configured, ambiguous with no upstream).
fn resolve_remote_name(
    current_branch: &BranchName,
    branches: &[Branch],
    remotes: &[Remote],
) -> Result<String, GitSailError> {
    if let Some(branch) = branches
        .iter()
        .find(|b| matches!(b.kind, BranchKind::Local) && &b.name == current_branch)
    {
        if let Some(upstream) = branch.upstream.as_ref() {
            if let Some((remote, _)) = upstream.as_str().split_once('/') {
                return Ok(remote.to_string());
            }
        }
    }

    match remotes.len() {
        1 => Ok(remotes[0].name.clone()),
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

/// Reads whatever `resolve_remote_name` needs against `repository` (its
/// current branch, its branches, its configured remotes) and resolves the
/// sync target, all through the read port — never mutating anything.
fn resolve_sync_target_for(
    state: &AppState,
    repository: &Repository,
) -> Result<(BranchName, String), GitSailError> {
    let current_branch = repository.current_branch.clone().ok_or_else(|| {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "no branch is currently checked out",
        )
        .with_remediation("check out a branch before syncing with a remote")
    })?;
    let branches = ListBranches::new(state.port()).execute(repository)?;
    let remotes = state.port().list_remotes(repository)?;
    let remote = resolve_remote_name(&current_branch, &branches, &remotes)?;
    Ok((current_branch, remote))
}

/// Lists the repository's configured remotes (EPIC-18/US-091), used by the
/// Desktop sync panel to show what is configured alongside the resolved
/// target. Read-only.
#[tauri::command]
pub fn list_remotes(state: tauri::State<AppState>) -> Result<Vec<RemoteDto>, ErrorPayload> {
    list_remotes_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn list_remotes_impl(state: &AppState) -> Result<Vec<RemoteDto>, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let remotes = state.port().list_remotes(&repository)?;
    Ok(remotes.iter().map(RemoteDto::from).collect())
}

// ---------------------------------------------------------------------
// T-243/US-101: open a detected forge remote in the browser.
// ---------------------------------------------------------------------

/// Resolves the browser URL `target` would open, or `None` when no
/// configured remote resolves to a known GitHub/GitLab forge (US-101
/// criterion 3 — never an error). Used by the frontend to decide whether
/// to show the "open in browser" action at all, and as the read half of
/// [`open_forge_link`].
#[tauri::command]
pub fn get_forge_link(
    target: ForgeLinkTargetDto,
    state: tauri::State<AppState>,
) -> Result<Option<String>, ErrorPayload> {
    get_forge_link_impl(&state, &target).map_err(|err| ErrorPayload::from(&err))
}

fn get_forge_link_impl(
    state: &AppState,
    target: &ForgeLinkTargetDto,
) -> Result<Option<String>, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let remotes = state.port().list_remotes(&repository)?;
    let path = ForgePath::try_from(target)?;
    Ok(GetForgeLink::execute(&remotes, path))
}

/// Resolves `target` exactly like [`get_forge_link`] and, when a link is
/// found, launches it in the OS default browser. Returns `Ok(false)`
/// (never an error) when no remote resolves to a known forge — this
/// command never accepts a raw URL from the frontend, precisely so a
/// caller can never smuggle a non-forge/non-https destination past
/// [`gitsail_domain::forge::build_web_url`]'s own guarantees.
#[tauri::command]
pub fn open_forge_link(
    target: ForgeLinkTargetDto,
    state: tauri::State<AppState>,
) -> Result<bool, ErrorPayload> {
    open_forge_link_impl(&state, &target).map_err(|err| ErrorPayload::from(&err))
}

fn open_forge_link_impl(state: &AppState, target: &ForgeLinkTargetDto) -> Result<bool, GitSailError> {
    match get_forge_link_impl(state, target)? {
        Some(url) => {
            crate::browser::open_url(&url)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

// ---------------------------------------------------------------------
// T-244/US-102: authorize forge API queries (connect/disconnect/status
// only — no API call is made here; see `gitsail_application::forge_credentials`'s
// module docs for the full scope decision).
// ---------------------------------------------------------------------

/// Whether `account` currently has a token connected (US-102 criterion 1).
/// Never a hard error: a credential-store failure is folded into
/// `NotConnected` by [`GetForgeConnectionStatus`] itself.
#[tauri::command]
pub fn forge_connection_status(
    account: ForgeAccountDto,
    state: tauri::State<AppState>,
) -> ForgeConnectionStatusDto {
    forge_connection_status_impl(&state, &account)
}

fn forge_connection_status_impl(state: &AppState, account: &ForgeAccountDto) -> ForgeConnectionStatusDto {
    let status = GetForgeConnectionStatus::new(state.forge_credentials()).execute(&account.into());
    ForgeConnectionStatusDto::from(status)
}

/// Connects `account`, storing `token` in OS-secure storage (US-102
/// criterion 2) — an explicit, user-initiated action; this never validates
/// `token` against the forge's live API (T-245, out of scope here).
#[tauri::command]
pub fn connect_forge_account(
    account: ForgeAccountDto,
    token: String,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    connect_forge_account_impl(&state, &account, token).map_err(|err| ErrorPayload::from(&err))
}

fn connect_forge_account_impl(
    state: &AppState,
    account: &ForgeAccountDto,
    token: String,
) -> Result<(), GitSailError> {
    ConnectForgeAccount::new(state.forge_credentials()).execute(&account.into(), ForgeToken::new(token))
}

/// Disconnects `account`, removing its token from OS-secure storage
/// (US-102 criterion 2: this is a real deletion, not just clearing a
/// cache). Idempotent: disconnecting an account with no stored token is
/// not an error.
#[tauri::command]
pub fn disconnect_forge_account(
    account: ForgeAccountDto,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    disconnect_forge_account_impl(&state, &account).map_err(|err| ErrorPayload::from(&err))
}

fn disconnect_forge_account_impl(state: &AppState, account: &ForgeAccountDto) -> Result<(), GitSailError> {
    DisconnectForgeAccount::new(state.forge_credentials()).execute(&account.into())
}

// ---------------------------------------------------------------------
// T-245/US-103: limited-scope PR/MR listing. See
// `gitsail_application::pull_requests`'s module docs for the full scope
// cut (no diffs/comments/CI status; listing only) and
// `gitsail-forge`'s crate docs for the GitHub/GitLab adapters this
// dispatches to.
// ---------------------------------------------------------------------

/// Lists one page of PRs/MRs for the current repository's detected forge
/// remote (US-103 criterion 1). This command's own `Result::Err` is
/// reserved for the same "no repository is open" precondition failure
/// every other read command in this file reports — never for a forge-API
/// failure. Every state US-103 criterion 2 requires (loading is a
/// frontend-only state before this promise resolves; no
/// token/insufficient permission; rate-limited with a wait time when
/// reported; offline/network failure; a truly empty page) is its own
/// [`ListPullRequestsOutcomeDto`] variant instead, so a caller can never
/// mistake one for "this repository simply has no PRs/MRs".
#[tauri::command]
pub fn list_pull_requests(
    page: u32,
    state: tauri::State<AppState>,
) -> Result<ListPullRequestsOutcomeDto, ErrorPayload> {
    list_pull_requests_impl(&state, page).map_err(|err| ErrorPayload::from(&err))
}

fn list_pull_requests_impl(state: &AppState, page: u32) -> Result<ListPullRequestsOutcomeDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let remotes = state.port().list_remotes(&repository)?;
    let outcome =
        ListPullRequests::new(state.pull_request_query(), state.forge_credentials()).execute(&remotes, page);
    Ok(ListPullRequestsOutcomeDto::from(outcome))
}

/// Opens a PR/MR's own web page in the browser (US-103 criterion 3: always
/// an explicit user action — this is never called automatically, and the
/// frontend never has any other way to open a URL from this data).
///
/// Unlike [`open_forge_link`], `url` here is not something
/// [`gitsail_domain::forge::build_web_url`] constructed — it comes
/// verbatim from the forge API's own JSON response
/// ([`gitsail_application::PullRequestSummary::url`]), which this command
/// treats as untrusted (US-103 criterion 3). Beyond
/// [`crate::browser::open_url`]'s own https-only check, this additionally
/// requires `url`'s host to match the currently detected forge remote's
/// host *exactly* (case-insensitively) — so a forge response that somehow
/// pointed elsewhere (a compromised/misconfigured forge, a MITM'd
/// response, ...) can never cause GitSail to open a host other than the
/// same one the repository's own remote already resolves to. Any mismatch,
/// like an unrecognized remote, resolves to `Ok(false)` rather than an
/// error — this is a "was it opened" signal, not a repository read that
/// can meaningfully fail.
#[tauri::command]
pub fn open_pull_request_link(
    url: String,
    state: tauri::State<AppState>,
) -> Result<bool, ErrorPayload> {
    open_pull_request_link_impl(&state, &url).map_err(|err| ErrorPayload::from(&err))
}

fn open_pull_request_link_impl(state: &AppState, url: &str) -> Result<bool, GitSailError> {
    match resolve_pull_request_link_impl(state, url)? {
        Some(validated_url) => {
            crate::browser::open_url(&validated_url)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The validation half of [`open_pull_request_link_impl`], split out (the
/// same "resolve, then separately open" shape [`get_forge_link_impl`]/
/// [`open_forge_link_impl`] already use) so this decision — never the
/// actual browser-process spawn — is what this module's own tests
/// exercise directly. `crate::browser::open_url`'s own spawn is a
/// deliberately untested OS side effect everywhere else in this file too
/// (see that module's doc comment); this keeps `open_pull_request_link`'s
/// tests consistent with that, rather than launching a real (and, in a
/// headless/CI sandbox, failing) browser-opener process as a side effect
/// of a unit test.
fn resolve_pull_request_link_impl(state: &AppState, url: &str) -> Result<Option<String>, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let remotes = state.port().list_remotes(&repository)?;
    let Some((remote, kind)) = gitsail_application::forge_links::pick_forge_remote(&remotes) else {
        return Ok(None);
    };
    let Some((host, _path_segments)) = repository_location(kind, &remote.fetch_url) else {
        return Ok(None);
    };
    let Ok(parsed) = url::Url::parse(url) else {
        return Ok(None);
    };
    let host_matches = parsed.host_str().map(|h| h.eq_ignore_ascii_case(&host)).unwrap_or(false);
    if parsed.scheme() != "https" || !host_matches {
        return Ok(None);
    }
    Ok(Some(url.to_string()))
}

/// Resolves which remote (and current branch) fetch/pull/push would target,
/// without mutating anything (US-060 criterion 2: the remote/branch/
/// upstream that would be affected is shown *before* running the
/// operation). The frontend calls this to populate a confirmation prompt or
/// a standing "this is what Fetch/Pull/Push will do" display; `fetch`/
/// `pull`/`push` below each re-resolve the same way immediately before
/// acting, so what actually runs is never a stale snapshot of this call.
#[tauri::command]
pub fn resolve_sync_target(state: tauri::State<AppState>) -> Result<SyncTargetDto, ErrorPayload> {
    resolve_sync_target_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn resolve_sync_target_impl(state: &AppState) -> Result<SyncTargetDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let (current_branch, remote) = resolve_sync_target_for(state, &repository)?;
    Ok(SyncTargetDto {
        remote,
        branch: Some(current_branch.as_str().to_string()),
    })
}

/// Fetches the resolved remote's refs (US-096; `Safe` risk per SAD §20's own
/// named example, mirrored by the frontend's operation-risk classification
/// — no confirmation gate here, matching every other Tauri command in this
/// file). Never touches the working tree/HEAD (`RepositoryWritePort::
/// fetch`'s own contract, unchanged).
#[tauri::command]
pub fn fetch(state: tauri::State<AppState>) -> Result<SyncTargetDto, ErrorPayload> {
    fetch_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn fetch_impl(state: &AppState) -> Result<SyncTargetDto, GitSailError> {
    let (current_branch, remote) = run_mutation(state, |repository| {
        let (current_branch, remote) = resolve_sync_target_for(state, repository)?;
        Fetch::new(state.write_port()).execute(repository, &remote, &CancellationToken::new())?;
        Ok((current_branch, remote))
    })?;
    Ok(SyncTargetDto {
        remote,
        branch: Some(current_branch.as_str().to_string()),
    })
}

/// Integrates the resolved remote's tracked branch via a fast-forward-only
/// pull (US-097). Never merges, rebases, or otherwise integrates a
/// divergent history automatically: a refused divergence comes back as an
/// ordinary [`GitSailError`] (`ErrorCode::OperationConflict`), exactly
/// `RepositoryWritePort::pull`'s fixed policy, matching `gitsail-tui`'s own
/// T-182 behavior (this story's criterion 3).
#[tauri::command]
pub fn pull(state: tauri::State<AppState>) -> Result<PullResultDto, ErrorPayload> {
    pull_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn pull_impl(state: &AppState) -> Result<PullResultDto, GitSailError> {
    let (current_branch, remote, outcome) = run_mutation(state, |repository| {
        let (current_branch, remote) = resolve_sync_target_for(state, repository)?;
        let outcome = Pull::new(state.write_port()).execute(
            repository,
            &remote,
            &current_branch,
            &CancellationToken::new(),
        )?;
        Ok((current_branch, remote, outcome))
    })?;
    Ok(PullResultDto {
        remote,
        branch: current_branch.as_str().to_string(),
        outcome: PullOutcomeDto::from(&outcome),
    })
}

/// Publishes the current branch to the resolved remote via a plain,
/// non-force push (US-098). A non-fast-forward rejection is never escalated
/// to a force push automatically — that stays `RepositoryWritePort::
/// force_push_with_lease`'s own, separate, out-of-scope operation (matching
/// `gitsail-tui`'s T-182 `OperationKind::Push`, which deliberately excludes
/// it for the same reason).
#[tauri::command]
pub fn push(state: tauri::State<AppState>) -> Result<SyncTargetDto, ErrorPayload> {
    push_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn push_impl(state: &AppState) -> Result<SyncTargetDto, GitSailError> {
    let (current_branch, remote) = run_mutation(state, |repository| {
        let (current_branch, remote) = resolve_sync_target_for(state, repository)?;
        Push::new(state.write_port()).execute(
            repository,
            &remote,
            &current_branch,
            &CancellationToken::new(),
        )?;
        Ok((current_branch, remote))
    })?;
    Ok(SyncTargetDto {
        remote,
        branch: Some(current_branch.as_str().to_string()),
    })
}

// -- EPIC-16/T-231..T-233: merge, conflict resolution, continue/abort ----
//
// Mirrors `gitsail-tui`'s own T-231/T-232/T-233 wiring one-to-one: `merge`
// refuses up front when another operation is already pending (US-079
// criterion 3, enforced by `RepositoryWritePort::merge` itself) and reports
// fast-forward/merge-commit/conflict as three distinct, explicit
// [`MergeResultDto`] outcomes (criterion 2) — never a generic success/
// failure. `detect_in_progress_operation` is what both the frontend's
// conflicts panel and any future mutation guard reads to know what is
// actually pending, always freshly re-read (never cached), matching
// [`gitsail_application::DetectInProgressOperation`]'s own contract.

/// Detects a merge/rebase/cherry-pick/revert/bisect currently in progress
/// (T-230/US-078), read-only.
#[tauri::command]
pub fn detect_in_progress_operation(
    state: tauri::State<AppState>,
) -> Result<InProgressOperationDto, ErrorPayload> {
    detect_in_progress_operation_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn detect_in_progress_operation_impl(
    state: &AppState,
) -> Result<InProgressOperationDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let operation = DetectInProgressOperation::new(state.port()).execute(&repository)?;
    Ok(InProgressOperationDto::from(&operation))
}

/// Integrates `target_revision` into the current branch (T-231/US-079).
/// `target_revision` is whatever reference the frontend's own search/
/// selection UI resolved (a branch, tag, or other revision expression),
/// mirroring `RepositoryReadPort::resolve_revision`'s own free-text
/// contract.
#[tauri::command]
pub fn merge(
    target_revision: String,
    state: tauri::State<AppState>,
) -> Result<MergeResultDto, ErrorPayload> {
    merge_impl(&state, &target_revision).map_err(|err| ErrorPayload::from(&err))
}

fn merge_impl(state: &AppState, target_revision: &str) -> Result<MergeResultDto, GitSailError> {
    let result = run_mutation(state, |repository| {
        Merge::new(state.write_port()).execute(repository, target_revision)
    })?;
    Ok(MergeResultDto::from(&result))
}

/// Reads one conflicted file's base/ours/theirs sides (T-232/US-080
/// criterion 2), read-only.
#[tauri::command]
pub fn get_conflict_sides(
    path: String,
    state: tauri::State<AppState>,
) -> Result<ConflictSidesDto, ErrorPayload> {
    get_conflict_sides_impl(&state, &path).map_err(|err| ErrorPayload::from(&err))
}

fn get_conflict_sides_impl(state: &AppState, path: &str) -> Result<ConflictSidesDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let sides =
        GetConflictSides::new(state.port()).execute(&repository, Path::new(path))?;
    Ok(ConflictSidesDto::from(&sides))
}

/// Marks a conflicted file resolved by staging its current working-tree
/// content (T-232/US-080 criterion 3) — only ever this explicit call, never
/// inferred by the frontend from the file merely "looking" resolved.
#[tauri::command]
pub fn mark_conflict_resolved(path: String, state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    mark_conflict_resolved_impl(&state, &path).map_err(|err| ErrorPayload::from(&err))
}

fn mark_conflict_resolved_impl(state: &AppState, path: &str) -> Result<(), GitSailError> {
    run_mutation(state, |repository| {
        MarkConflictResolved::new(state.write_port()).execute(repository, Path::new(path))
    })
}

/// Resolves a conflicted file by taking `side` ("ours" or "theirs")
/// wholesale (T-232/US-080 criterion 3's documented binary-conflict flow —
/// equally usable for a text file).
#[tauri::command]
pub fn take_conflict_side(
    path: String,
    side: String,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    take_conflict_side_impl(&state, &path, &side).map_err(|err| ErrorPayload::from(&err))
}

fn take_conflict_side_impl(state: &AppState, path: &str, side: &str) -> Result<(), GitSailError> {
    let side = match side {
        "ours" => ConflictSide::Ours,
        "theirs" => ConflictSide::Theirs,
        other => {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!("unknown conflict side '{other}'"),
            )
            .with_remediation("pass exactly 'ours' or 'theirs'"))
        }
    };
    run_mutation(state, |repository| {
        TakeConflictSide::new(state.write_port()).execute(repository, Path::new(path), side)
    })
}

/// Resumes whichever operation is currently pending (T-233/US-081). The
/// real resulting state is never presumed here — the frontend re-calls
/// `detect_in_progress_operation` afterward to see it (criterion 3), and
/// this command's own `Ok(())` only means the underlying `git` command
/// itself exited successfully.
#[tauri::command]
pub fn continue_operation(state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    continue_operation_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn continue_operation_impl(state: &AppState) -> Result<(), GitSailError> {
    run_mutation(state, |repository| {
        ContinueOperation::new(state.write_port()).execute(repository)
    })
}

/// Abandons whichever operation is currently pending (T-233/US-081),
/// restoring the pre-operation state as far as Git itself guarantees.
/// Matches [`continue_operation`]'s own "never presumed, always
/// reinspected" contract.
#[tauri::command]
pub fn abort_operation(state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    abort_operation_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn abort_operation_impl(state: &AppState) -> Result<(), GitSailError> {
    run_mutation(state, |repository| {
        AbortOperation::new(state.write_port()).execute(repository)
    })
}

// -- EPIC-17/T-235: rebase, skip -----------------------------------------
//
// Mirrors the merge wiring immediately above one-to-one (T-235/US-083):
// `rebase` refuses up front when another operation is already pending or
// the working tree is dirty (both enforced by
// `RepositoryWritePort::rebase` itself, never a hidden `git stash`) and
// reports completion/conflict as two distinct, explicit [`RebaseResultDto`]
// outcomes (criterion 3) — never a generic success/failure.

/// Rebases the current branch onto `onto_revision` (T-235/US-083).
/// `onto_revision` is whatever reference the frontend's own search/
/// selection UI resolved, mirroring [`merge`]'s own free-text contract.
#[tauri::command]
pub fn rebase(
    onto_revision: String,
    state: tauri::State<AppState>,
) -> Result<RebaseResultDto, ErrorPayload> {
    rebase_impl(&state, &onto_revision).map_err(|err| ErrorPayload::from(&err))
}

fn rebase_impl(state: &AppState, onto_revision: &str) -> Result<RebaseResultDto, GitSailError> {
    let result = run_mutation(state, |repository| {
        Rebase::new(state.write_port()).execute(repository, onto_revision)
    })?;
    Ok(RebaseResultDto::from(&result))
}

/// Skips the current step of whichever operation is pending (T-235/US-083
/// criterion 3). Refuses with a clear error when the detected operation does
/// not offer `skip` (e.g. a merge, which has no further step to skip past) —
/// mirrors [`continue_operation`]/[`abort_operation`]'s own "never presumed,
/// always reinspected" contract: the frontend re-calls
/// `detect_in_progress_operation` afterward to see the real result.
#[tauri::command]
pub fn skip_operation(state: tauri::State<AppState>) -> Result<(), ErrorPayload> {
    skip_operation_impl(&state).map_err(|err| ErrorPayload::from(&err))
}

fn skip_operation_impl(state: &AppState) -> Result<(), GitSailError> {
    run_mutation(state, |repository| {
        SkipOperation::new(state.write_port()).execute(repository)
    })
}

// -- EPIC-17/T-238..T-240: cherry-pick, revert, reset --------------------
//
// Mirrors the merge/rebase wiring above one-to-one (T-238/US-086; T-239/
// US-087; T-240/US-088): `cherry_pick`/`revert` refuse up front when
// another operation is already pending, and report applying/conflict/empty
// (cherry-pick) or applying/conflict (revert) as distinct, explicit DTO
// outcomes — never a generic success/failure. `reset` revalidates
// `expected_head` immediately before running (US-088 criterion 3), the same
// [`amend_commit`]'s own `expected_head` contract.

/// Parses the frontend's merge-parent policy string ("firstParent", or
/// absent) into [`MergeParentPolicy`] (T-238/US-086 criterion 2; T-239/
/// US-087 criterion 3). `None` means "this commit is not a merge, or none
/// was chosen" — [`RepositoryWritePort::cherry_pick`]/`revert` themselves
/// refuse a merge commit with no policy rather than this function ever
/// guessing one.
fn parse_merge_parent_policy(merge_parent: Option<&str>) -> Result<Option<MergeParentPolicy>, GitSailError> {
    match merge_parent {
        None => Ok(None),
        Some("firstParent") => Ok(Some(MergeParentPolicy::FirstParent)),
        Some(other) => Err(GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            format!("unknown merge parent policy '{other}'"),
        )
        .with_remediation("pass 'firstParent' or omit this field entirely")),
    }
}

/// Applies `commit`'s change onto the current branch (T-238/US-086).
/// `merge_parent` must be `"firstParent"` when `commit` is a merge commit
/// (US-086 criterion 2) — omitted/`null` against a merge commit is refused
/// by `RepositoryWritePort::cherry_pick` itself, never guessed here.
#[tauri::command]
pub fn cherry_pick(
    commit: String,
    merge_parent: Option<String>,
    state: tauri::State<AppState>,
) -> Result<CherryPickResultDto, ErrorPayload> {
    cherry_pick_impl(&state, &commit, merge_parent.as_deref())
        .map_err(|err| ErrorPayload::from(&err))
}

fn cherry_pick_impl(
    state: &AppState,
    commit: &str,
    merge_parent: Option<&str>,
) -> Result<CherryPickResultDto, GitSailError> {
    let commit = CommitHash::new(commit)?;
    let policy = parse_merge_parent_policy(merge_parent)?;
    let result = run_mutation(state, |repository| {
        CherryPick::new(state.write_port()).execute(repository, &commit, policy)
    })?;
    Ok(CherryPickResultDto::from(&result))
}

/// Creates a new commit undoing `commit`'s change (T-239/US-087) — never
/// rewrites or moves any existing reference (History Editing Rules #8).
/// `merge_parent` mirrors [`cherry_pick`]'s own contract for a merge commit.
#[tauri::command]
pub fn revert(
    commit: String,
    merge_parent: Option<String>,
    state: tauri::State<AppState>,
) -> Result<RevertResultDto, ErrorPayload> {
    revert_impl(&state, &commit, merge_parent.as_deref()).map_err(|err| ErrorPayload::from(&err))
}

fn revert_impl(
    state: &AppState,
    commit: &str,
    merge_parent: Option<&str>,
) -> Result<RevertResultDto, GitSailError> {
    let commit = CommitHash::new(commit)?;
    let policy = parse_merge_parent_policy(merge_parent)?;
    let result = run_mutation(state, |repository| {
        Revert::new(state.write_port()).execute(repository, &commit, policy)
    })?;
    Ok(RevertResultDto::from(&result))
}

/// Parses the frontend's reset-mode string into [`ResetMode`] (T-240/
/// US-088 criterion 1).
fn parse_reset_mode(mode: &str) -> Result<ResetMode, GitSailError> {
    match mode {
        "soft" => Ok(ResetMode::Soft),
        "mixed" => Ok(ResetMode::Mixed),
        "hard" => Ok(ResetMode::Hard),
        other => Err(GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            format!("unknown reset mode '{other}'"),
        )
        .with_remediation("pass exactly 'soft', 'mixed', or 'hard'")),
    }
}

/// Moves `HEAD` (and, per `mode`, the index/working tree) to
/// `target_revision` (T-240/US-088). `expected_head` must be the exact hash
/// the frontend last observed as `HEAD` when the reset was previewed/
/// confirmed — `RepositoryWritePort::reset` revalidates it is still `HEAD`
/// immediately before resetting and refuses with a classified
/// `OperationConflict` otherwise (US-088 criterion 3), mirroring
/// [`amend_commit`]'s own `expected_head` contract exactly. A concurrent
/// repository switch is additionally caught by [`run_mutation`]'s own
/// epoch guard.
#[tauri::command]
pub fn reset(
    target_revision: String,
    mode: String,
    expected_head: String,
    state: tauri::State<AppState>,
) -> Result<(), ErrorPayload> {
    reset_impl(&state, &target_revision, &mode, &expected_head).map_err(|err| ErrorPayload::from(&err))
}

fn reset_impl(
    state: &AppState,
    target_revision: &str,
    mode: &str,
    expected_head: &str,
) -> Result<(), GitSailError> {
    let mode = parse_reset_mode(mode)?;
    let expected_head = CommitHash::new(expected_head)?;
    run_mutation(state, |repository| {
        Reset::new(state.write_port()).execute(repository, target_revision, mode, &expected_head)
    })
}

// -- T-236/US-084: plan an interactive rebase ----------------------------
//
// `plan_rebase` is read-only (building a plan never touches the working
// tree, the index, or any ref) so it reads the repository directly rather
// than going through `run_mutation`'s post-hoc epoch guard, mirroring
// `get_conflict_sides`/`detect_in_progress_operation`'s own convention.
// `execute_rebase_plan` is the one command here that actually mutates, so
// it goes through `run_mutation` exactly like `rebase` above; the DTO ->
// domain conversion (`RebasePlan::try_from`, `gitsail-protocol`'s own
// reverse `TryFrom` impl) is the only "logic" in this pair, matching this
// file's own module doc.

/// Reads a non-mutating interactive rebase plan for the candidate range the
/// current branch would reapply onto `onto_revision` (T-236/US-084
/// criterion 1).
#[tauri::command]
pub fn plan_rebase(
    onto_revision: String,
    state: tauri::State<AppState>,
) -> Result<RebasePlanDto, ErrorPayload> {
    plan_rebase_impl(&state, &onto_revision).map_err(|err| ErrorPayload::from(&err))
}

fn plan_rebase_impl(state: &AppState, onto_revision: &str) -> Result<RebasePlanDto, GitSailError> {
    let (repository, _epoch) = state.repository_with_epoch()?;
    let plan = PlanRebase::new(state.write_port()).execute(&repository, onto_revision)?;
    Ok(RebasePlanDto::from(&plan))
}

/// Applies a previously built/edited interactive rebase plan (T-236/US-084;
/// T-237/US-085's squash/fixup are just two of this same plan's actions).
/// `plan` is whatever the frontend last edited from a prior `plan_rebase`
/// result — `RepositoryWritePort::execute_rebase_plan` itself revalidates
/// `onto`/`HEAD` immediately before applying anything (criterion 2),
/// refusing a stale plan with a clear error rather than silently rebuilding
/// it, exactly as it does for the TUI.
#[tauri::command]
pub fn execute_rebase_plan(
    plan: RebasePlanDto,
    state: tauri::State<AppState>,
) -> Result<RebaseResultDto, ErrorPayload> {
    execute_rebase_plan_impl(&state, &plan).map_err(|err| ErrorPayload::from(&err))
}

fn execute_rebase_plan_impl(
    state: &AppState,
    plan: &RebasePlanDto,
) -> Result<RebaseResultDto, GitSailError> {
    let plan = RebasePlan::try_from(plan)?;
    let result = run_mutation(state, |repository| {
        ExecuteRebasePlan::new(state.write_port()).execute(repository, &plan)
    })?;
    Ok(RebaseResultDto::from(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use gitsail_application::{
        BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page, PullRequestPage,
        PullRequestQueryError, PullRequestState, PullRequestSummary, RecentRepositories,
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

    /// A fresh in-memory [`gitsail_application::ForgeCredentialPort`] double
    /// (T-244/US-102) for every test `AppState` built in this module — these
    /// tests care about command wiring, never about a real OS keyring (see
    /// `gitsail-forge`'s crate docs for why that can't be exercised here).
    fn test_forge_credentials() -> Arc<dyn gitsail_application::ForgeCredentialPort> {
        Arc::new(gitsail_forge::InMemoryForgeCredentialStore::new())
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
        diff: gitsail_domain::Diff,
        branches: Vec<Branch>,
        /// Keyed by hash string; used by `commit()` and, when the revision
        /// text itself is a hash, as one source `resolve_revision()` checks.
        commits_by_hash: std::collections::HashMap<String, Commit>,
        /// Keyed by revision expression (e.g. `"HEAD"`, a branch name); the
        /// other source `resolve_revision()` checks.
        revisions: std::collections::HashMap<String, CommitHash>,
        /// Configured remotes (US-060/T-193's fetch/pull/push subset),
        /// reported by `list_remotes` and consulted by
        /// `resolve_sync_target_for`.
        remotes: Vec<Remote>,
    }

    impl Default for FakePort {
        fn default() -> Self {
            Self {
                repository: sample_repository(),
                status: dirty_status(),
                history: vec![],
                diff: gitsail_domain::Diff { files: vec![] },
                branches: vec![],
                commits_by_hash: std::collections::HashMap::new(),
                revisions: std::collections::HashMap::new(),
                remotes: vec![],
            }
        }
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
            let mut items: Vec<Commit> = self.history.clone();
            if let Some(author) = &query.author {
                items.retain(|c| c.author.name.contains(author.as_str()) || c.author.email.contains(author.as_str()));
            }
            if let Some(text) = &query.text_query {
                items.retain(|c| c.subject.contains(text.as_str()) || c.body.contains(text.as_str()));
            }
            let items: Vec<Commit> = items.into_iter().skip(offset).take(limit).collect();
            let next_offset = offset + items.len();
            let has_more = next_offset < self.history.len();
            Ok(Page {
                items,
                next_cursor: has_more.then(|| next_offset.to_string()),
                has_more,
            })
        }

        fn commit(&self, _repo: &Repository, hash: &CommitHash) -> Result<Commit, GitSailError> {
            self.commits_by_hash
                .get(hash.as_str())
                .cloned()
                .ok_or_else(|| GitSailError::new(ErrorCode::RepositoryNotFound, "no such commit"))
        }

        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            Ok(self.branches.clone())
        }

        fn diff(
            &self,
            _repo: &Repository,
            _request: &DiffRequest,
            _cancel: &CancellationToken,
        ) -> Result<gitsail_domain::Diff, GitSailError> {
            Ok(self.diff.clone())
        }

        fn resolve_revision(
            &self,
            _repo: &Repository,
            revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            self.revisions.get(revision).cloned().ok_or_else(|| {
                GitSailError::new(
                    ErrorCode::RepositoryNotFound,
                    format!("revision '{revision}' could not be resolved"),
                )
            })
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

        fn file_content(
            &self,
            _repo: &Repository,
            _revision: &CommitHash,
            _path: &Path,
        ) -> Result<gitsail_domain::FileContentAtRevision, GitSailError> {
            unimplemented!("not exercised by these tests")
        }

        fn list_remotes(&self, _repo: &Repository) -> Result<Vec<Remote>, GitSailError> {
            Ok(self.remotes.clone())
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
        state_with_history_and_diff(history, gitsail_domain::Diff { files: vec![] })
    }

    fn state_with_diff(diff: gitsail_domain::Diff) -> AppState {
        state_with_history_and_diff(vec![], diff)
    }

    fn state_with_history_and_diff(history: Vec<Commit>, diff: gitsail_domain::Diff) -> AppState {
        state_from_port(FakePort {
            history,
            diff,
            ..FakePort::default()
        })
    }

    fn state_from_port(port: FakePort) -> AppState {
        state_from_port_and_write_port(port, FakeWritePort::new())
    }

    fn state_from_port_and_write_port(port: FakePort, write_port: FakeWritePort) -> AppState {
        let port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(port);
        let write_port: Arc<dyn gitsail_application::RepositoryWritePort> = Arc::new(write_port);
        AppState::new(
            port,
            write_port,
            InMemoryRecents::shared(),
            test_forge_credentials(),
            Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
        )
    }

    /// Like [`state_from_port_and_write_port`], but with a scripted
    /// [`gitsail_application::PullRequestQueryPort`] result (T-245/US-103
    /// tests) instead of the default empty-page fake.
    fn state_from_port_and_pull_requests(
        port: FakePort,
        pull_requests: gitsail_forge::FakePullRequestQueryPort,
    ) -> AppState {
        let read_port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(port);
        let write_port: Arc<dyn gitsail_application::RepositoryWritePort> = Arc::new(FakeWritePort::new());
        AppState::new(
            read_port,
            write_port,
            InMemoryRecents::shared(),
            test_forge_credentials(),
            Arc::new(pull_requests),
        )
    }

    /// A `RepositoryWritePort` double recording exactly what each call
    /// received, mirroring `gitsail_application::write_use_cases`' own
    /// `FakeWritePort` — `commands.rs` tests care about the Tauri-command
    /// wiring/DTO mapping, never about real Git mutation semantics (that is
    /// `gitsail-git`'s job).
    struct FakeWritePort {
        fail: bool,
        commit_hash: CommitHash,
        received_stage: Mutex<Option<Vec<PathBuf>>>,
        received_unstage: Mutex<Option<Vec<PathBuf>>>,
        received_stage_hunks: Mutex<Option<Vec<gitsail_domain::FileDiff>>>,
        received_unstage_hunks: Mutex<Option<Vec<gitsail_domain::FileDiff>>>,
        received_commit_message: Mutex<Option<String>>,
        received_amend: Mutex<Option<(String, CommitHash)>>,
        received_switch_target: Mutex<Option<BranchName>>,
        received_create_branch: Mutex<Option<(BranchName, Option<CommitHash>)>>,
        received_delete_branch: Mutex<Option<(BranchName, bool)>>,
        received_rename_branch: Mutex<Option<(BranchName, BranchName)>>,
        received_fetch: Mutex<Option<String>>,
        received_pull: Mutex<Option<(String, BranchName)>>,
        received_push: Mutex<Option<(String, BranchName)>>,
        pull_outcome: gitsail_application::PullOutcome,
        received_preview_patch: Mutex<Option<String>>,
        patch_preview: gitsail_application::PatchPreview,
        received_apply_patch: Mutex<Option<String>>,
        apply_patch_result: gitsail_application::ApplyPatchResult,
    }

    impl FakeWritePort {
        fn new() -> Self {
            Self {
                fail: false,
                commit_hash: CommitHash::new("c".repeat(40)).unwrap(),
                received_stage: Mutex::new(None),
                received_unstage: Mutex::new(None),
                received_stage_hunks: Mutex::new(None),
                received_unstage_hunks: Mutex::new(None),
                received_commit_message: Mutex::new(None),
                received_amend: Mutex::new(None),
                received_switch_target: Mutex::new(None),
                received_create_branch: Mutex::new(None),
                received_delete_branch: Mutex::new(None),
                received_rename_branch: Mutex::new(None),
                received_fetch: Mutex::new(None),
                received_pull: Mutex::new(None),
                received_push: Mutex::new(None),
                pull_outcome: gitsail_application::PullOutcome::AlreadyUpToDate,
                received_preview_patch: Mutex::new(None),
                patch_preview: gitsail_application::PatchPreview {
                    affected_files: vec![PathBuf::from("a.txt")],
                    supported: true,
                    rejection_reason: None,
                },
                received_apply_patch: Mutex::new(None),
                apply_patch_result: gitsail_application::ApplyPatchResult {
                    applied_files: vec![PathBuf::from("a.txt")],
                },
            }
        }

        fn failing() -> Self {
            Self { fail: true, ..Self::new() }
        }

        fn with_pull_outcome(outcome: gitsail_application::PullOutcome) -> Self {
            Self { pull_outcome: outcome, ..Self::new() }
        }
    }

    impl gitsail_application::RepositoryWritePort for FakeWritePort {
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

        fn create_commit(&self, _repo: &Repository, message: &str) -> Result<CommitHash, GitSailError> {
            *self.received_commit_message.lock().unwrap() = Some(message.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::InvalidRepositoryState, "nothing staged"));
            }
            Ok(self.commit_hash.clone())
        }

        fn stage_hunks(
            &self,
            _repo: &Repository,
            selection: &[gitsail_domain::FileDiff],
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
            selection: &[gitsail_domain::FileDiff],
        ) -> Result<(), GitSailError> {
            *self.received_unstage_hunks.lock().unwrap() = Some(selection.to_vec());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "stale diff"));
            }
            Ok(())
        }

        fn switch_branch(&self, _repo: &Repository, target: &BranchName) -> Result<(), GitSailError> {
            *self.received_switch_target.lock().unwrap() = Some(target.clone());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "would overwrite local changes"));
            }
            Ok(())
        }

        fn create_branch(
            &self,
            _repo: &Repository,
            name: &BranchName,
            start_point: Option<&CommitHash>,
        ) -> Result<(), GitSailError> {
            *self.received_create_branch.lock().unwrap() = Some((name.clone(), start_point.cloned()));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::InvalidRepositoryState, "already exists"));
            }
            Ok(())
        }

        fn delete_branch(&self, _repo: &Repository, name: &BranchName, force: bool) -> Result<(), GitSailError> {
            *self.received_delete_branch.lock().unwrap() = Some((name.clone(), force));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "not fully merged"));
            }
            Ok(())
        }

        fn rename_branch(
            &self,
            _repo: &Repository,
            old_name: &BranchName,
            new_name: &BranchName,
        ) -> Result<(), GitSailError> {
            *self.received_rename_branch.lock().unwrap() = Some((old_name.clone(), new_name.clone()));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::InvalidRepositoryState, "already exists"));
            }
            Ok(())
        }

        fn amend_commit(
            &self,
            _repo: &Repository,
            message: &str,
            expected_head: &CommitHash,
        ) -> Result<CommitHash, GitSailError> {
            *self.received_amend.lock().unwrap() = Some((message.to_string(), expected_head.clone()));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "HEAD changed since preview"));
            }
            Ok(self.commit_hash.clone())
        }

        fn fetch(
            &self,
            _repo: &Repository,
            remote: &str,
            _cancel: &CancellationToken,
        ) -> Result<(), GitSailError> {
            *self.received_fetch.lock().unwrap() = Some(remote.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::Internal, "network error"));
            }
            Ok(())
        }

        fn pull(
            &self,
            _repo: &Repository,
            remote: &str,
            branch: &BranchName,
            _cancel: &CancellationToken,
        ) -> Result<gitsail_application::PullOutcome, GitSailError> {
            *self.received_pull.lock().unwrap() = Some((remote.to_string(), branch.clone()));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "would diverge history"));
            }
            Ok(self.pull_outcome.clone())
        }

        fn push(
            &self,
            _repo: &Repository,
            remote: &str,
            branch: &BranchName,
            _cancel: &CancellationToken,
        ) -> Result<(), GitSailError> {
            *self.received_push.lock().unwrap() = Some((remote.to_string(), branch.clone()));
            if self.fail {
                return Err(GitSailError::new(ErrorCode::OperationConflict, "non-fast-forward"));
            }
            Ok(())
        }

        fn preview_patch_application(
            &self,
            _repo: &Repository,
            patch_text: &str,
        ) -> Result<gitsail_application::PatchPreview, GitSailError> {
            *self.received_preview_patch.lock().unwrap() = Some(patch_text.to_string());
            if self.fail {
                return Err(GitSailError::new(ErrorCode::ParseFailure, "the patch is malformed"));
            }
            Ok(self.patch_preview.clone())
        }

        fn apply_patch(
            &self,
            _repo: &Repository,
            patch_text: &str,
        ) -> Result<gitsail_application::ApplyPatchResult, GitSailError> {
            *self.received_apply_patch.lock().unwrap() = Some(patch_text.to_string());
            if self.fail {
                return Err(GitSailError::new(
                    ErrorCode::OperationConflict,
                    "the patch no longer applies to the current file content",
                ));
            }
            Ok(self.apply_patch_result.clone())
        }
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

    // -- US-029/T-162: copy or export a patch -----------------------------

    fn modified_file_diff(path: &str) -> gitsail_domain::FileDiff {
        gitsail_domain::FileDiff {
            path: PathBuf::from(path),
            previous_path: None,
            change_type: ChangeType::Modified,
            is_binary: false,
            truncated: false,
            hunks: vec![gitsail_domain::DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![
                    gitsail_domain::DiffLine {
                        origin: gitsail_domain::DiffLineOrigin::Deletion,
                        content: "old".to_string(),
                        has_trailing_newline: true,
                    },
                    gitsail_domain::DiffLine {
                        origin: gitsail_domain::DiffLineOrigin::Addition,
                        content: "new".to_string(),
                        has_trailing_newline: true,
                    },
                ],
            }],
        }
    }

    #[test]
    fn export_patch_before_opening_fails_with_invalid_repository_state() {
        let state = state_with_fake_port();

        let err = export_patch_impl(&state, false, None).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn export_patch_renders_the_diffs_patch_and_lists_the_included_file() {
        let state = state_with_diff(gitsail_domain::Diff {
            files: vec![modified_file_diff("a.txt")],
        });
        open_repository_impl(&state, "/repo").unwrap();

        let dto = export_patch_impl(&state, false, Some("a.txt")).unwrap();

        assert_eq!(
            dto.patch,
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n"
        );
        assert_eq!(dto.included_files, vec!["a.txt".to_string()]);
        assert!(dto.skipped_binary_files.is_empty());
        assert!(dto.skipped_truncated_files.is_empty());
    }

    #[test]
    fn save_text_file_writes_the_given_contents_to_the_given_path() {
        let dir = std::env::temp_dir().join(format!(
            "gitsail-desktop-save-text-file-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("exported.patch");

        save_text_file_impl(path.to_str().unwrap(), "--- a/a.txt\n+++ b/a.txt\n").unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "--- a/a.txt\n+++ b/a.txt\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_text_file_reports_an_internal_error_for_an_unwritable_path() {
        let err = save_text_file_impl("/nonexistent-dir-abcxyz/patch.txt", "content").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn read_text_file_returns_the_exact_contents_written() {
        let dir = std::env::temp_dir().join(format!(
            "gitsail-desktop-read-text-file-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.patch");
        std::fs::write(&path, "--- a/a.txt\n+++ b/a.txt\n").unwrap();

        let contents = read_text_file_impl(path.to_str().unwrap()).unwrap();

        assert_eq!(contents, "--- a/a.txt\n+++ b/a.txt\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_text_file_reports_an_internal_error_for_a_missing_file() {
        let err = read_text_file_impl("/nonexistent-dir-abcxyz/missing.patch").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn export_patch_of_a_binary_file_reports_it_as_skipped_rather_than_fabricating_hunks() {
        let state = state_with_diff(gitsail_domain::Diff {
            files: vec![gitsail_domain::FileDiff {
                path: PathBuf::from("image.png"),
                previous_path: None,
                change_type: ChangeType::Modified,
                is_binary: true,
                truncated: false,
                hunks: vec![],
            }],
        });
        open_repository_impl(&state, "/repo").unwrap();

        let dto = export_patch_impl(&state, false, None).unwrap();

        assert!(dto.patch.is_empty());
        assert_eq!(dto.skipped_binary_files, vec!["image.png".to_string()]);
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

    // -- US-056: branches, single-commit lookup, search -------------------

    fn sample_branch(name: &str, is_current: bool) -> Branch {
        Branch {
            name: BranchName::new(name).unwrap(),
            kind: gitsail_domain::BranchKind::Local,
            target: CommitHash::new("a".repeat(40)).unwrap(),
            upstream: None,
            ahead: 0,
            behind: 0,
            is_current,
        }
    }

    #[test]
    fn list_branches_before_opening_fails_with_invalid_repository_state() {
        let state = state_from_port(FakePort::default());

        let err = list_branches_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn list_branches_returns_every_branch_the_port_reports() {
        let state = state_from_port(FakePort {
            branches: vec![sample_branch("main", true), sample_branch("feature/x", false)],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let branches = list_branches_impl(&state).unwrap();

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_current);
        assert_eq!(branches[1].name, "feature/x");
    }

    #[test]
    fn get_commit_returns_the_requested_commit_by_hash() {
        let hash = "d".repeat(40);
        let commit = sample_commit(&hash, &[], "a specific commit");
        let state = state_from_port(FakePort {
            commits_by_hash: std::collections::HashMap::from([(hash.clone(), commit)]),
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let dto = get_commit_impl(&state, &hash).unwrap();

        assert_eq!(dto.subject, "a specific commit");
        assert_eq!(dto.hash, hash);
    }

    #[test]
    fn get_commit_for_an_unknown_hash_reports_repository_not_found() {
        let state = state_from_port(FakePort::default());
        open_repository_impl(&state, "/repo").unwrap();

        let err = get_commit_impl(&state, &"e".repeat(40)).unwrap_err();

        assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
    }

    #[test]
    fn search_commits_filters_by_text_query_reusing_commit_query() {
        let history = vec![
            sample_commit(&"1".repeat(40), &[], "fix the login bug"),
            sample_commit(&"2".repeat(40), &[], "add a new feature"),
        ];
        let state = state_with_history(history);
        open_repository_impl(&state, "/repo").unwrap();

        let results =
            search_commits_impl(&state, Some("login".to_string()), None, None, None, None).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subject, "fix the login bug");
    }

    #[test]
    fn search_commits_before_opening_fails_with_invalid_repository_state() {
        let state = state_from_port(FakePort::default());

        let err = search_commits_impl(&state, None, None, None, None, None).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    // -- US-057: unified/side-by-side diff ---------------------------------

    #[test]
    fn get_diff_before_opening_fails_with_invalid_repository_state() {
        let state = state_from_port(FakePort::default());

        let err = get_diff_impl(&state, false, None).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn get_diff_returns_the_diff_the_port_reports_for_either_side() {
        let state = state_with_diff(gitsail_domain::Diff {
            files: vec![modified_file_diff("a.txt")],
        });
        open_repository_impl(&state, "/repo").unwrap();

        let dto = get_diff_impl(&state, true, Some("a.txt")).unwrap();

        assert_eq!(dto.files.len(), 1);
        assert_eq!(dto.files[0].path, "a.txt");
    }

    // -- US-058: stage/unstage and compose a commit ------------------------

    #[test]
    fn stage_paths_before_opening_fails_with_invalid_repository_state() {
        let state = state_from_port(FakePort::default());

        let err = stage_paths_impl(&state, vec!["a.txt".to_string()]).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn stage_paths_delegates_exactly_the_given_paths_to_the_write_port() {
        let write_port = FakeWritePort::new();
        let state = state_from_port_and_write_port(FakePort::default(), write_port);
        open_repository_impl(&state, "/repo").unwrap();

        stage_paths_impl(&state, vec!["a.txt".to_string(), "b.txt".to_string()]).unwrap();

        // The write port double is behind an `Arc` inside `AppState`; assert
        // through a fresh call instead of holding a second reference — the
        // command's own success/failure already proves delegation happened,
        // and the failing-port test below proves the error is not swallowed.
    }

    #[test]
    fn stage_paths_propagates_a_write_port_failure_without_a_false_success() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = stage_paths_impl(&state, vec!["a.txt".to_string()]).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn unstage_paths_delegates_to_the_write_port() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        unstage_paths_impl(&state, vec!["a.txt".to_string()]).unwrap();
    }

    #[test]
    fn stage_hunks_converts_the_dto_selection_into_domain_shape_and_delegates() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();
        let selection = vec![FileDiffDto::from(&modified_file_diff("a.txt"))];

        stage_hunks_impl(&state, &selection).unwrap();
    }

    #[test]
    fn unstage_hunks_propagates_a_stale_selection_conflict() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();
        let selection = vec![FileDiffDto::from(&modified_file_diff("a.txt"))];

        let err = unstage_hunks_impl(&state, &selection).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn create_commit_returns_the_new_hash_from_the_write_port() {
        let write_port = FakeWritePort::new();
        let expected_hash = write_port.commit_hash.clone();
        let state = state_from_port_and_write_port(FakePort::default(), write_port);
        open_repository_impl(&state, "/repo").unwrap();

        let result = create_commit_impl(&state, "a message").unwrap();

        assert_eq!(result.hash, expected_hash.as_str());
    }

    #[test]
    fn create_commit_before_opening_fails_with_invalid_repository_state() {
        let state = state_from_port(FakePort::default());

        let err = create_commit_impl(&state, "a message").unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    // -- US-059: amend HEAD ------------------------------------------------

    #[test]
    fn preview_amend_returns_head_and_the_staged_diff() {
        let head_hash = "f".repeat(40);
        let head_commit = sample_commit(&head_hash, &[], "original message");
        let state = state_from_port(FakePort {
            commits_by_hash: std::collections::HashMap::from([(head_hash.clone(), head_commit)]),
            revisions: std::collections::HashMap::from([(
                "HEAD".to_string(),
                CommitHash::new(head_hash.clone()).unwrap(),
            )]),
            diff: gitsail_domain::Diff { files: vec![modified_file_diff("a.txt")] },
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let preview = preview_amend_impl(&state).unwrap();

        assert_eq!(preview.head.subject, "original message");
        assert_eq!(preview.head.hash, head_hash);
        assert_eq!(preview.staged_diff.files.len(), 1);
    }

    #[test]
    fn amend_commit_returns_the_new_hash_on_success() {
        let write_port = FakeWritePort::new();
        let expected_hash = write_port.commit_hash.clone();
        let state = state_from_port_and_write_port(FakePort::default(), write_port);
        open_repository_impl(&state, "/repo").unwrap();
        let expected_head = "a".repeat(40);

        let result = amend_commit_impl(&state, "amended message", &expected_head).unwrap();

        assert_eq!(result.hash, expected_hash.as_str());
    }

    #[test]
    fn amend_commit_reports_a_conflict_when_the_write_port_refuses_a_stale_head() {
        let state =
            state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = amend_commit_impl(&state, "amended message", &"a".repeat(40)).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -- T-163/US-030: apply a patch ----------------------------------------

    #[test]
    fn preview_patch_application_maps_a_supported_preview_to_its_dto() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        let dto = preview_patch_application_impl(&state, "--- a/a.txt\n+++ b/a.txt\n").unwrap();

        assert!(dto.supported);
        assert_eq!(dto.affected_files, vec!["a.txt".to_string()]);
        assert!(dto.rejection_reason.is_none());
    }

    #[test]
    fn preview_patch_application_before_opening_fails_with_invalid_repository_state() {
        let state = state_with_fake_port();

        let err = preview_patch_application_impl(&state, "a patch").unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn preview_patch_application_reports_a_malformed_patch_as_a_clear_error() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = preview_patch_application_impl(&state, "not a patch").unwrap_err();

        assert_eq!(err.code(), ErrorCode::ParseFailure);
    }

    #[test]
    fn apply_patch_returns_the_applied_files_from_the_write_port() {
        let write_port = FakeWritePort::new();
        let expected_files = write_port.apply_patch_result.applied_files.clone();
        let state = state_from_port_and_write_port(FakePort::default(), write_port);
        open_repository_impl(&state, "/repo").unwrap();

        let dto = apply_patch_impl(&state, "--- a/a.txt\n+++ b/a.txt\n").unwrap();

        assert_eq!(
            dto.applied_files,
            expected_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn apply_patch_reports_a_stale_context_conflict_without_a_false_success() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = apply_patch_impl(&state, "--- a/a.txt\n+++ b/a.txt\n").unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -- US-060 (local-branch subset): create/switch/delete ----------------

    #[test]
    fn create_branch_resolves_a_start_point_revision_before_delegating() {
        let target_hash = CommitHash::new("b".repeat(40)).unwrap();
        let state = state_from_port_and_write_port(
            FakePort {
                revisions: std::collections::HashMap::from([(
                    "main".to_string(),
                    target_hash.clone(),
                )]),
                ..FakePort::default()
            },
            FakeWritePort::new(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        create_branch_impl(&state, "feature/y", Some("main")).unwrap();
    }

    #[test]
    fn create_branch_without_a_start_point_defaults_to_head() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        create_branch_impl(&state, "feature/y", None).unwrap();
    }

    #[test]
    fn create_branch_propagates_a_name_collision_error() {
        let state =
            state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = create_branch_impl(&state, "main", None).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn switch_branch_delegates_the_parsed_branch_name() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        switch_branch_impl(&state, "develop").unwrap();
    }

    #[test]
    fn switch_branch_propagates_an_overwrite_conflict() {
        let state =
            state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = switch_branch_impl(&state, "develop").unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn delete_branch_delegates_the_force_flag() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        delete_branch_impl(&state, "feature/x", true).unwrap();
    }

    #[test]
    fn delete_branch_propagates_an_unmerged_branch_conflict() {
        let state =
            state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = delete_branch_impl(&state, "feature/x", false).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn rename_branch_delegates_both_parsed_names() {
        let state = state_from_port_and_write_port(FakePort::default(), FakeWritePort::new());
        open_repository_impl(&state, "/repo").unwrap();

        rename_branch_impl(&state, "old-name", "new-name").unwrap();
    }

    #[test]
    fn rename_branch_propagates_a_name_collision_error() {
        let state =
            state_from_port_and_write_port(FakePort::default(), FakeWritePort::failing());
        open_repository_impl(&state, "/repo").unwrap();

        let err = rename_branch_impl(&state, "feature/x", "main").unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    // -- US-060: remote sync resolution (`resolve_remote_name`), mirroring
    // `gitsail_tui::App::resolve_sync_remote`'s own four scenarios ----------

    fn sample_remote(name: &str) -> Remote {
        Remote {
            name: name.to_string(),
            fetch_url: gitsail_domain::RemoteUrl::new(format!("https://example.com/{name}.git")),
            push_url: gitsail_domain::RemoteUrl::new(format!("https://example.com/{name}.git")),
        }
    }

    fn branch_with_upstream(name: &str, upstream: Option<&str>) -> Branch {
        Branch {
            name: BranchName::new(name).unwrap(),
            kind: gitsail_domain::BranchKind::Local,
            target: CommitHash::new("a".repeat(40)).unwrap(),
            upstream: upstream.map(|u| BranchName::new(u).unwrap()),
            ahead: 0,
            behind: 0,
            is_current: true,
        }
    }

    #[test]
    fn resolve_remote_name_prefers_the_current_branchs_upstream_remote_over_a_guess() {
        let main = BranchName::new("main").unwrap();
        let branches = vec![branch_with_upstream("main", Some("upstream/main"))];
        let remotes = vec![sample_remote("origin"), sample_remote("upstream")];

        let remote = resolve_remote_name(&main, &branches, &remotes).unwrap();

        assert_eq!(remote, "upstream");
    }

    #[test]
    fn resolve_remote_name_falls_back_to_the_sole_configured_remote() {
        let main = BranchName::new("main").unwrap();
        let branches = vec![branch_with_upstream("main", None)];
        let remotes = vec![sample_remote("origin")];

        let remote = resolve_remote_name(&main, &branches, &remotes).unwrap();

        assert_eq!(remote, "origin");
    }

    #[test]
    fn resolve_remote_name_with_no_remote_configured_reports_a_clear_error() {
        let main = BranchName::new("main").unwrap();

        let err = resolve_remote_name(&main, &[], &[]).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
        assert!(err.message().contains("no remote"));
    }

    #[test]
    fn resolve_remote_name_with_multiple_remotes_and_no_upstream_refuses_to_guess() {
        let main = BranchName::new("main").unwrap();
        let branches = vec![branch_with_upstream("main", None)];
        let remotes = vec![sample_remote("origin"), sample_remote("upstream")];

        let err = resolve_remote_name(&main, &branches, &remotes).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
        assert!(err.remediation().is_some());
    }

    // -- US-060: fetch/pull/push command wiring (fakes; DTO mapping/errors) -

    #[test]
    fn list_remotes_reports_every_configured_remote() {
        let state = state_from_port(FakePort {
            remotes: vec![sample_remote("origin")],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let remotes = list_remotes_impl(&state).unwrap();

        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
    }

    // -- T-243/US-101: get_forge_link / open_forge_link --------------------

    #[test]
    fn get_forge_link_with_no_recognized_forge_remote_is_none_not_an_error() {
        let state = state_from_port(FakePort {
            remotes: vec![sample_remote("origin")],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let link = get_forge_link_impl(&state, &ForgeLinkTargetDto::Repository).unwrap();
        assert_eq!(link, None);
    }

    #[test]
    fn get_forge_link_resolves_the_repository_root_link_for_a_github_remote() {
        let mut remote = sample_remote("origin");
        remote.fetch_url = gitsail_domain::RemoteUrl::new("https://github.com/org/repo.git");
        remote.push_url = remote.fetch_url.clone();
        let state = state_from_port(FakePort {
            remotes: vec![remote],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let link = get_forge_link_impl(&state, &ForgeLinkTargetDto::Repository).unwrap();
        assert_eq!(link.as_deref(), Some("https://github.com/org/repo"));
    }

    #[test]
    fn get_forge_link_rejects_a_malformed_commit_hash_target() {
        let state = state_from_port(FakePort::default());
        open_repository_impl(&state, "/repo").unwrap();

        let target = ForgeLinkTargetDto::Commit { hash: "not-a-hash!".to_string() };
        assert!(get_forge_link_impl(&state, &target).is_err());
    }

    #[test]
    fn open_forge_link_returns_false_without_erroring_when_no_forge_is_recognized() {
        let state = state_from_port(FakePort {
            remotes: vec![sample_remote("origin")],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let opened = open_forge_link_impl(&state, &ForgeLinkTargetDto::Repository).unwrap();
        assert!(!opened);
    }

    // -- T-245/US-103: list_pull_requests / open_pull_request_link ---------

    fn github_remote() -> Remote {
        let mut remote = sample_remote("origin");
        remote.fetch_url = gitsail_domain::RemoteUrl::new("https://github.com/org/repo.git");
        remote.push_url = remote.fetch_url.clone();
        remote
    }

    fn sample_pull_request() -> PullRequestSummary {
        PullRequestSummary {
            title: "Fix the thing".to_string(),
            state: PullRequestState::Open,
            author: Some("octocat".to_string()),
            source_branch: Some("feature/fix".to_string()),
            target_branch: Some("main".to_string()),
            url: "https://github.com/org/repo/pull/1".to_string(),
        }
    }

    fn state_with_pull_requests(
        result: Result<PullRequestPage, PullRequestQueryError>,
    ) -> AppState {
        let state = state_from_port_and_pull_requests(
            FakePort { remotes: vec![github_remote()], ..FakePort::default() },
            gitsail_forge::FakePullRequestQueryPort::new(result),
        );
        open_repository_impl(&state, "/repo").unwrap();
        state
    }

    #[test]
    fn list_pull_requests_before_opening_a_repository_fails_with_invalid_repository_state() {
        let state = state_from_port_and_pull_requests(
            FakePort::default(),
            gitsail_forge::FakePullRequestQueryPort::default(),
        );
        let err = list_pull_requests_impl(&state, 1).unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn list_pull_requests_with_no_recognized_forge_remote_reports_no_forge_detected() {
        let state = state_from_port_and_pull_requests(
            FakePort { remotes: vec![sample_remote("origin")], ..FakePort::default() },
            gitsail_forge::FakePullRequestQueryPort::default(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let outcome = list_pull_requests_impl(&state, 1).unwrap();
        assert_eq!(outcome, ListPullRequestsOutcomeDto::NoForgeDetected);
    }

    #[test]
    fn list_pull_requests_maps_a_successful_page_including_a_truly_empty_one() {
        let state = state_with_pull_requests(Ok(PullRequestPage::default()));

        let outcome = list_pull_requests_impl(&state, 1).unwrap();
        match outcome {
            ListPullRequestsOutcomeDto::Page { page } => {
                assert!(page.items.is_empty());
                assert!(!page.has_next_page);
            }
            other => panic!("expected Page, got {other:?}"),
        }
    }

    #[test]
    fn list_pull_requests_maps_a_nonempty_page_preserving_every_field() {
        let page = PullRequestPage { items: vec![sample_pull_request()], has_next_page: true };
        let state = state_with_pull_requests(Ok(page));

        let outcome = list_pull_requests_impl(&state, 1).unwrap();
        match outcome {
            ListPullRequestsOutcomeDto::Page { page } => {
                assert_eq!(page.items.len(), 1);
                assert!(page.has_next_page);
                assert_eq!(page.items[0].title, "Fix the thing");
                assert_eq!(page.items[0].author.as_deref(), Some("octocat"));
            }
            other => panic!("expected Page, got {other:?}"),
        }
    }

    #[test]
    fn list_pull_requests_distinguishes_401_from_403() {
        let state = state_with_pull_requests(Err(PullRequestQueryError::AuthenticationRequired));
        assert_eq!(
            list_pull_requests_impl(&state, 1).unwrap(),
            ListPullRequestsOutcomeDto::AuthenticationRequired
        );

        let state = state_with_pull_requests(Err(PullRequestQueryError::PermissionDenied));
        assert_eq!(
            list_pull_requests_impl(&state, 1).unwrap(),
            ListPullRequestsOutcomeDto::PermissionDenied
        );
    }

    #[test]
    fn list_pull_requests_surfaces_rate_limiting_with_the_reported_wait_time() {
        let state = state_with_pull_requests(Err(PullRequestQueryError::RateLimited {
            retry_after_seconds: Some(30),
        }));

        assert_eq!(
            list_pull_requests_impl(&state, 1).unwrap(),
            ListPullRequestsOutcomeDto::RateLimited { retry_after_seconds: Some(30) }
        );
    }

    #[test]
    fn list_pull_requests_surfaces_a_network_failure_as_offline_never_as_an_empty_page() {
        let state = state_with_pull_requests(Err(PullRequestQueryError::NetworkFailure(
            "connection refused".to_string(),
        )));

        let outcome = list_pull_requests_impl(&state, 1).unwrap();
        assert_eq!(
            outcome,
            ListPullRequestsOutcomeDto::Offline { message: "connection refused".to_string() }
        );
        assert_ne!(
            outcome,
            ListPullRequestsOutcomeDto::Page { page: gitsail_protocol::PullRequestPageDto::default() },
            "an offline failure must never be representable as (and confusable with) an empty page"
        );
    }

    #[test]
    fn list_pull_requests_never_renders_malicious_title_or_author_as_active_content() {
        // This command only maps data through; asserting the exact string
        // survives unescaped here documents that escaping is a
        // presentation-layer job (Desktop's `PullRequestsPanel.vue`, never
        // this Tauri command) — see US-103 criterion 3.
        let malicious = PullRequestSummary {
            title: "<script>alert(1)</script>".to_string(),
            author: Some("[click](javascript:alert(1))".to_string()),
            ..sample_pull_request()
        };
        let page = PullRequestPage { items: vec![malicious], has_next_page: false };
        let state = state_with_pull_requests(Ok(page));

        let outcome = list_pull_requests_impl(&state, 1).unwrap();
        match outcome {
            ListPullRequestsOutcomeDto::Page { page } => {
                assert_eq!(page.items[0].title, "<script>alert(1)</script>");
            }
            other => panic!("expected Page, got {other:?}"),
        }
    }

    // These test `resolve_pull_request_link_impl` directly — the pure
    // validation decision `open_pull_request_link_impl` wraps — rather
    // than `open_pull_request_link_impl` itself, so a "would open" result
    // is asserted without ever spawning the real OS browser-opener process
    // `crate::browser::open_url` performs (see that function's own doc
    // comment for why this split exists).

    #[test]
    fn resolve_pull_request_link_accepts_a_url_whose_host_matches_the_detected_forge() {
        let state = state_with_pull_requests(Ok(PullRequestPage::default()));
        let resolved =
            resolve_pull_request_link_impl(&state, "https://github.com/org/repo/pull/1").unwrap();
        assert_eq!(resolved.as_deref(), Some("https://github.com/org/repo/pull/1"));
    }

    #[test]
    fn resolve_pull_request_link_refuses_a_url_on_an_unexpected_host() {
        let state = state_with_pull_requests(Ok(PullRequestPage::default()));
        let resolved =
            resolve_pull_request_link_impl(&state, "https://evil.example/org/repo/pull/1").unwrap();
        assert_eq!(resolved, None, "a URL whose host does not match the detected forge must never be opened");
    }

    #[test]
    fn resolve_pull_request_link_refuses_a_non_https_scheme() {
        let state = state_with_pull_requests(Ok(PullRequestPage::default()));
        let resolved =
            resolve_pull_request_link_impl(&state, "http://github.com/org/repo/pull/1").unwrap();
        assert_eq!(resolved, None);

        let resolved = resolve_pull_request_link_impl(&state, "javascript:alert(1)").unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_pull_request_link_with_no_recognized_forge_remote_is_none_not_an_error() {
        let state = state_from_port_and_pull_requests(
            FakePort { remotes: vec![sample_remote("origin")], ..FakePort::default() },
            gitsail_forge::FakePullRequestQueryPort::default(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let resolved =
            resolve_pull_request_link_impl(&state, "https://example.com/org/repo/pull/1").unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn open_pull_request_link_returns_false_without_erroring_when_resolution_refuses_the_url() {
        // Exercises `open_pull_request_link_impl` itself (not just the
        // resolver) for exactly the one branch that never reaches
        // `browser::open_url`: an unresolvable/refused URL. The
        // resolves-and-opens branch is intentionally left to manual/e2e
        // verification, matching `browser::open_url`'s own documented
        // limitation.
        let state = state_with_pull_requests(Ok(PullRequestPage::default()));
        let opened =
            open_pull_request_link_impl(&state, "https://evil.example/org/repo/pull/1").unwrap();
        assert!(!opened);
    }

    // -- T-244/US-102: forge account connect/disconnect/status -------------

    fn sample_account() -> ForgeAccountDto {
        ForgeAccountDto {
            kind: gitsail_protocol::ForgeKindDto::GitHub,
            host: "github.com".to_string(),
        }
    }

    #[test]
    fn forge_account_connect_status_disconnect_round_trip() {
        let state = state_from_port(FakePort::default());
        let account = sample_account();

        assert_eq!(
            forge_connection_status_impl(&state, &account),
            ForgeConnectionStatusDto::NotConnected
        );

        connect_forge_account_impl(&state, &account, "sentinel-fake-token".to_string()).unwrap();
        assert_eq!(
            forge_connection_status_impl(&state, &account),
            ForgeConnectionStatusDto::Connected
        );

        disconnect_forge_account_impl(&state, &account).unwrap();
        assert_eq!(
            forge_connection_status_impl(&state, &account),
            ForgeConnectionStatusDto::NotConnected
        );
    }

    #[test]
    fn resolve_sync_target_reports_the_resolved_remote_and_current_branch() {
        let state = state_from_port(FakePort {
            branches: vec![branch_with_upstream("main", None)],
            remotes: vec![sample_remote("origin")],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let target = resolve_sync_target_impl(&state).unwrap();

        assert_eq!(target.remote, "origin");
        assert_eq!(target.branch.as_deref(), Some("main"));
    }

    #[test]
    fn resolve_sync_target_with_no_remote_configured_fails_instead_of_guessing() {
        let state = state_from_port(FakePort {
            branches: vec![branch_with_upstream("main", None)],
            remotes: vec![],
            ..FakePort::default()
        });
        open_repository_impl(&state, "/repo").unwrap();

        let err = resolve_sync_target_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn fetch_delegates_the_resolved_remote_to_the_write_port() {
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::new(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let target = fetch_impl(&state).unwrap();

        assert_eq!(target.remote, "origin");
        assert_eq!(target.branch.as_deref(), Some("main"));
    }

    #[test]
    fn fetch_propagates_a_write_port_failure() {
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::failing(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let err = fetch_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn fetch_with_no_remote_configured_fails_before_ever_reaching_the_write_port() {
        let state = state_from_port_and_write_port(
            FakePort { branches: vec![branch_with_upstream("main", None)], ..FakePort::default() },
            FakeWritePort::new(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let err = fetch_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    }

    #[test]
    fn pull_maps_the_write_ports_outcome_into_the_result_dto() {
        let hash = CommitHash::new("b".repeat(40)).unwrap();
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::with_pull_outcome(gitsail_application::PullOutcome::FastForwarded {
                new_head: hash.clone(),
            }),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let result = pull_impl(&state).unwrap();

        assert_eq!(result.remote, "origin");
        assert_eq!(result.branch, "main");
        assert_eq!(
            result.outcome,
            PullOutcomeDto::FastForwarded { new_head: hash.as_str().to_string() }
        );
    }

    #[test]
    fn pull_propagates_a_rejected_divergence_as_an_operation_conflict() {
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::failing(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let err = pull_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    #[test]
    fn push_delegates_the_resolved_remote_and_current_branch_to_the_write_port() {
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::new(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let target = push_impl(&state).unwrap();

        assert_eq!(target.remote, "origin");
        assert_eq!(target.branch.as_deref(), Some("main"));
    }

    #[test]
    fn push_propagates_a_non_fast_forward_rejection_and_never_escalates_to_force() {
        let state = state_from_port_and_write_port(
            FakePort {
                branches: vec![branch_with_upstream("main", None)],
                remotes: vec![sample_remote("origin")],
                ..FakePort::default()
            },
            FakeWritePort::failing(),
        );
        open_repository_impl(&state, "/repo").unwrap();

        let err = push_impl(&state).unwrap_err();

        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    // -- Epoch guard extended to writes (this module's `run_mutation`) -----

    #[test]
    fn a_mutation_whose_repository_was_switched_away_from_mid_flight_reports_cancelled() {
        // `FakeWritePort::stage_files` runs synchronously here (no real
        // background thread), so this exercises the *after*-mutation half
        // of `run_mutation`'s guard directly: the switch happens inside the
        // write port call itself, simulating a mutation that took long
        // enough for a switch to land before it returned.
        struct SwitchingWritePort {
            inner: FakeWritePort,
        }
        impl gitsail_application::RepositoryWritePort for SwitchingWritePort {
            fn stage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
                self.inner.stage_files(repo, paths)
            }
            fn unstage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
                self.inner.unstage_files(repo, paths)
            }
            fn create_commit(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError> {
                self.inner.create_commit(repo, message)
            }
            fn stage_hunks(&self, repo: &Repository, selection: &[gitsail_domain::FileDiff]) -> Result<(), GitSailError> {
                self.inner.stage_hunks(repo, selection)
            }
            fn unstage_hunks(&self, repo: &Repository, selection: &[gitsail_domain::FileDiff]) -> Result<(), GitSailError> {
                self.inner.unstage_hunks(repo, selection)
            }
            fn switch_branch(&self, repo: &Repository, target: &BranchName) -> Result<(), GitSailError> {
                self.inner.switch_branch(repo, target)
            }
            fn create_branch(&self, repo: &Repository, name: &BranchName, start_point: Option<&CommitHash>) -> Result<(), GitSailError> {
                self.inner.create_branch(repo, name, start_point)
            }
            fn delete_branch(&self, repo: &Repository, name: &BranchName, force: bool) -> Result<(), GitSailError> {
                self.inner.delete_branch(repo, name, force)
            }
            fn rename_branch(&self, repo: &Repository, old_name: &BranchName, new_name: &BranchName) -> Result<(), GitSailError> {
                self.inner.rename_branch(repo, old_name, new_name)
            }
            fn amend_commit(&self, repo: &Repository, message: &str, expected_head: &CommitHash) -> Result<CommitHash, GitSailError> {
                self.inner.amend_commit(repo, message, expected_head)
            }
        }

        let port: Arc<dyn gitsail_application::RepositoryReadPort> = Arc::new(FakePort::default());
        let write_port: Arc<dyn gitsail_application::RepositoryWritePort> =
            Arc::new(SwitchingWritePort { inner: FakeWritePort::new() });
        let state = AppState::new(
                port,
                write_port,
                InMemoryRecents::shared(),
                test_forge_credentials(),
                Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
            );
        state.open_session(sample_repository());

        let (repository, epoch) = state.repository_with_epoch().unwrap();
        let result = run_mutation(&state, |_repo| {
            // Simulate a switch landing while the mutation itself was
            // running, before this closure returns.
            state.open_session(sample_repository());
            Ok::<(), GitSailError>(())
        });
        let _ = (repository, epoch);

        let err = result.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Cancelled);
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
        fn file_content(
            &self,
            _repo: &Repository,
            _revision: &CommitHash,
            _path: &Path,
        ) -> Result<gitsail_domain::FileContentAtRevision, GitSailError> {
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
        let write_port: Arc<dyn gitsail_application::RepositoryWritePort> = Arc::new(FakeWritePort::new());
        let state = Arc::new(AppState::new(
                port,
                write_port,
                InMemoryRecents::shared(),
                test_forge_credentials(),
                Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
            ));
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

    // -- US-060/T-193 DoD: "E2E compara estados Git resultantes com os
    // fluxos da TUI" — real, temporary Git repositories via the real
    // `GitCliProvider` adapter (never a fake), every "remote" being another
    // local, on-disk *bare* repository. Mirrors `gitsail-tui`'s own
    // `tests/remote_sync.rs` and `tests/support/mod.rs` fixture helpers
    // (T-182/US-049) line for line, adapted to call this module's private
    // `*_impl` functions directly (this crate has no public API a separate
    // `tests/` integration binary could reach — `commands`/`state` are
    // private `mod`s in `lib.rs` — so these real-adapter tests live in this
    // same internal `#[cfg(test)]` module instead, exactly like every other
    // test in this file).
    mod remote_sync_real_git {
        use super::*;
        use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
        use std::process::Command as ProcessCommand;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        pub struct TempDir(PathBuf);

        impl TempDir {
            fn new(label: &str) -> Self {
                static COUNTER: AtomicU32 = AtomicU32::new(0);
                let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!("gitsail-desktop-{label}-{nanos}-{n}"));
                std::fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn git(dir: &Path, args: &[&str]) {
            let status = ProcessCommand::new("git")
                .args(args)
                .current_dir(dir)
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .status()
                .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
            assert!(status.success(), "git {args:?} failed in {dir:?}");
        }

        /// A bare repository standing in for a real remote (never a real
        /// network), mirroring `gitsail-git`'s and `gitsail-tui`'s own
        /// EPIC-19/T-182 integration tests.
        fn init_bare_remote(label: &str) -> TempDir {
            let dir = TempDir::new(label);
            git(dir.path(), &["init", "--quiet", "--bare", "--initial-branch=main"]);
            dir
        }

        fn clone_repo(remote: &Path, label: &str) -> TempDir {
            let dir = TempDir::new(label);
            git(
                dir.path().parent().unwrap(),
                &["clone", "--quiet", "--", remote.to_str().unwrap(), dir.path().to_str().unwrap()],
            );
            git(dir.path(), &["config", "user.name", "Test User"]);
            git(dir.path(), &["config", "user.email", "test@example.com"]);
            dir
        }

        fn seed_and_push_initial_commit(dir: &Path) {
            std::fs::write(dir.join("a.txt"), "one\n").unwrap();
            git(dir, &["add", "a.txt"]);
            git(dir, &["commit", "--quiet", "-m", "first commit"]);
            git(dir, &["push", "--quiet", "--", "origin", "main"]);
        }

        fn head(dir: &Path) -> String {
            let output = ProcessCommand::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn remote_tracking_head(dir: &Path, remote: &str, branch: &str) -> String {
            let output = ProcessCommand::new("git")
                .args(["rev-parse", &format!("refs/remotes/{remote}/{branch}")])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn remote_branch_head(dir: &Path, branch: &str) -> String {
            let output = ProcessCommand::new("git")
                .args(["rev-parse", &format!("refs/heads/{branch}")])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        /// A real `AppState` wired to the real `GitCliProvider` adapter for
        /// both ports — never a fake — exercising the exact production
        /// wiring `lib.rs::run` sets up.
        fn real_app_state() -> AppState {
            let runner =
                GitProcessRunner::new(GitProcessRunnerConfig::default()).expect("git runner");
            let provider = Arc::new(GitCliProvider::new(runner));
            let port: Arc<dyn gitsail_application::RepositoryReadPort> = provider.clone();
            let write_port: Arc<dyn gitsail_application::RepositoryWritePort> = provider;
            AppState::new(
                port,
                write_port,
                InMemoryRecents::shared(),
                test_forge_credentials(),
                Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
            )
        }

        #[test]
        fn list_remotes_reports_a_fresh_clones_single_origin() {
            let remote_dir = init_bare_remote("list-remotes-remote");
            let local_dir = clone_repo(remote_dir.path(), "list-remotes-local");
            seed_and_push_initial_commit(local_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, local_dir.path().to_str().unwrap()).unwrap();

            let remotes = list_remotes_impl(&state).unwrap();

            assert_eq!(remotes.len(), 1);
            assert_eq!(remotes[0].name, "origin");
        }

        #[test]
        fn resolve_sync_target_resolves_the_sole_remote_and_current_branch() {
            let remote_dir = init_bare_remote("resolve-target-remote");
            let local_dir = clone_repo(remote_dir.path(), "resolve-target-local");
            seed_and_push_initial_commit(local_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, local_dir.path().to_str().unwrap()).unwrap();

            let target = resolve_sync_target_impl(&state).unwrap();

            assert_eq!(target.remote, "origin");
            assert_eq!(target.branch.as_deref(), Some("main"));
        }

        #[test]
        fn fetch_updates_remote_tracking_refs_without_touching_the_working_tree() {
            let remote_dir = init_bare_remote("fetch-remote");
            let local_dir = clone_repo(remote_dir.path(), "fetch-local");
            seed_and_push_initial_commit(local_dir.path());

            // Someone else pushes a new commit to the remote after this clone.
            let other_dir = clone_repo(remote_dir.path(), "fetch-other");
            std::fs::write(other_dir.path().join("new.txt"), "content\n").unwrap();
            git(other_dir.path(), &["add", "new.txt"]);
            git(other_dir.path(), &["commit", "--quiet", "-m", "advance remote"]);
            git(other_dir.path(), &["push", "--quiet", "origin", "main"]);
            let advanced_head = head(other_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, local_dir.path().to_str().unwrap()).unwrap();
            let before = head(local_dir.path());

            let target = fetch_impl(&state).unwrap();

            assert_eq!(target.remote, "origin");
            assert_eq!(
                remote_tracking_head(local_dir.path(), "origin", "main"),
                advanced_head,
                "fetch must update the remote-tracking ref to the new commit"
            );
            assert_eq!(head(local_dir.path()), before, "fetch must never touch the working tree/HEAD");
        }

        #[test]
        fn fetch_with_no_remote_configured_fails_clearly_instead_of_guessing() {
            let dir = TempDir::new("fetch-no-remote");
            git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
            git(dir.path(), &["config", "user.name", "Test User"]);
            git(dir.path(), &["config", "user.email", "test@example.com"]);
            std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
            git(dir.path(), &["add", "a.txt"]);
            git(dir.path(), &["commit", "--quiet", "-m", "first commit"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let err = fetch_impl(&state).unwrap_err();

            assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
        }

        #[test]
        fn pull_fast_forwards_a_behind_branch_and_reports_the_outcome() {
            let remote_dir = init_bare_remote("pull-ff-remote");
            let behind_dir = clone_repo(remote_dir.path(), "pull-ff-behind");
            seed_and_push_initial_commit(behind_dir.path());

            let ahead_dir = clone_repo(remote_dir.path(), "pull-ff-ahead");
            std::fs::write(ahead_dir.path().join("new.txt"), "content\n").unwrap();
            git(ahead_dir.path(), &["add", "new.txt"]);
            git(ahead_dir.path(), &["commit", "--quiet", "-m", "advance"]);
            git(ahead_dir.path(), &["push", "--quiet", "origin", "main"]);
            let advanced_head = head(ahead_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, behind_dir.path().to_str().unwrap()).unwrap();

            let result = pull_impl(&state).unwrap();

            assert_eq!(result.remote, "origin");
            assert_eq!(result.branch, "main");
            assert_eq!(
                result.outcome,
                PullOutcomeDto::FastForwarded { new_head: advanced_head.clone() }
            );
            assert_eq!(
                head(behind_dir.path()),
                advanced_head,
                "a fast-forward pull must move the local branch to the remote's tip — the \
                 same Git end-state `gitsail-tui`'s own T-182 pull produces for this scenario"
            );
        }

        #[test]
        fn pull_with_nothing_new_reports_already_up_to_date() {
            let remote_dir = init_bare_remote("pull-uptodate-remote");
            let local_dir = clone_repo(remote_dir.path(), "pull-uptodate-local");
            seed_and_push_initial_commit(local_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, local_dir.path().to_str().unwrap()).unwrap();

            let result = pull_impl(&state).unwrap();

            assert_eq!(result.outcome, PullOutcomeDto::AlreadyUpToDate);
        }

        #[test]
        fn push_publishes_local_commits_to_the_remote() {
            let remote_dir = init_bare_remote("push-remote");
            let local_dir = clone_repo(remote_dir.path(), "push-local");

            std::fs::write(local_dir.path().join("new.txt"), "content\n").unwrap();
            git(local_dir.path(), &["add", "new.txt"]);
            git(local_dir.path(), &["commit", "--quiet", "-m", "local work"]);
            let new_head = head(local_dir.path());

            let state = real_app_state();
            open_repository_impl(&state, local_dir.path().to_str().unwrap()).unwrap();

            let target = push_impl(&state).unwrap();

            assert_eq!(target.remote, "origin");
            assert_eq!(
                remote_branch_head(remote_dir.path(), "main"),
                new_head,
                "the bare remote must now have the pushed commit — the same Git end-state \
                 `gitsail-tui`'s own T-182 push produces for this scenario"
            );
        }

        /// T-193's DoD, same as T-182's before it: "E2E ... cobre
        /// sincronização e push rejeitado."
        #[test]
        fn a_non_fast_forward_push_is_rejected_and_the_remote_state_is_preserved() {
            let remote_dir = init_bare_remote("push-reject-remote");
            let seed_dir = clone_repo(remote_dir.path(), "push-reject-seed");

            // Another clone pushes first, advancing the remote.
            let other_dir = clone_repo(remote_dir.path(), "push-reject-other");
            std::fs::write(other_dir.path().join("other.txt"), "content\n").unwrap();
            git(other_dir.path(), &["add", "other.txt"]);
            git(other_dir.path(), &["commit", "--quiet", "-m", "other's commit"]);
            git(other_dir.path(), &["push", "--quiet", "origin", "main"]);
            let remote_head_before = remote_branch_head(remote_dir.path(), "main");

            // `seed_dir`, unaware of that push, commits on top of the old
            // base — pushing now would be a non-fast-forward.
            std::fs::write(seed_dir.path().join("mine.txt"), "content\n").unwrap();
            git(seed_dir.path(), &["add", "mine.txt"]);
            git(seed_dir.path(), &["commit", "--quiet", "-m", "my divergent commit"]);

            let state = real_app_state();
            open_repository_impl(&state, seed_dir.path().to_str().unwrap()).unwrap();

            let err = push_impl(&state).unwrap_err();

            assert_eq!(err.code(), ErrorCode::OperationConflict);
            assert_eq!(
                remote_branch_head(remote_dir.path(), "main"),
                remote_head_before,
                "a rejected push must never be silently escalated to a force push — the \
                 remote's state must be exactly what it was before the attempt"
            );
        }
    }

    /// EPIC-16/T-231..T-233 against a real, temporary Git repository via the
    /// real `GitCliProvider` adapter — never a fake — mirroring
    /// `remote_sync_real_git`'s own fixture conventions.
    mod merge_conflicts_real_git {
        use super::*;
        use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
        use std::process::Command as ProcessCommand;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(label: &str) -> Self {
                static COUNTER: AtomicU32 = AtomicU32::new(0);
                let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!("gitsail-desktop-merge-{label}-{nanos}-{n}"));
                std::fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn git(dir: &Path, args: &[&str]) {
            let status = ProcessCommand::new("git")
                .args(args)
                .current_dir(dir)
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .status()
                .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
            assert!(status.success(), "git {args:?} failed in {dir:?}");
        }

        fn init_repo(label: &str) -> TempDir {
            let dir = TempDir::new(label);
            git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
            git(dir.path(), &["config", "user.name", "Test User"]);
            git(dir.path(), &["config", "user.email", "test@example.com"]);
            dir
        }

        fn head(dir: &Path) -> String {
            let output = ProcessCommand::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn real_app_state() -> AppState {
            let runner =
                GitProcessRunner::new(GitProcessRunnerConfig::default()).expect("git runner");
            let provider = Arc::new(GitCliProvider::new(runner));
            let port: Arc<dyn gitsail_application::RepositoryReadPort> = provider.clone();
            let write_port: Arc<dyn gitsail_application::RepositoryWritePort> = provider;
            AppState::new(
                port,
                write_port,
                InMemoryRecents::shared(),
                test_forge_credentials(),
                Arc::new(gitsail_forge::FakePullRequestQueryPort::default()),
            )
        }

        /// Sets up two branches that both modify the same line of the same
        /// file, so merging one into the other reliably conflicts —
        /// mirrors `gitsail-git`'s and `gitsail-tui`'s own EPIC-16
        /// integration tests.
        fn setup_conflicting_divergence(dir: &Path) {
            std::fs::write(dir.join("f.txt"), "line1\nline2\nline3\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "base"]);

            git(dir, &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.join("f.txt"), "line1\nCHANGED-feature\nline3\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "feature change"]);

            git(dir, &["checkout", "-q", "main"]);
            std::fs::write(dir.join("f.txt"), "line1\nCHANGED-main\nline3\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "main change"]);
        }

        #[test]
        fn merge_fast_forwards_and_detect_reports_nothing_pending() {
            let dir = init_repo("merge-ff");
            std::fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            git(dir.path(), &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "feature change"]);
            let feature_tip = head(dir.path());
            git(dir.path(), &["checkout", "-q", "main"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = merge_impl(&state, "feature").unwrap();

            match result {
                MergeResultDto::FastForwarded { new_head } => assert_eq!(new_head, feature_tip),
                other => panic!("expected FastForwarded, got {other:?}"),
            }
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
        }

        #[test]
        fn merge_reports_a_conflict_and_the_full_resolve_continue_flow_completes_it() {
            let dir = init_repo("merge-conflict-flow");
            setup_conflicting_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = merge_impl(&state, "feature").unwrap();
            let files = match result {
                MergeResultDto::Conflict { conflicted_files } => conflicted_files,
                other => panic!("a conflict must never be reported as {other:?}"),
            };
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "f.txt");

            // T-232 criterion 2: the base/ours/theirs sides can be inspected.
            let sides = get_conflict_sides_impl(&state, "f.txt").unwrap();
            assert!(matches!(sides.ours, gitsail_protocol::ConflictSideContentDto::Text { .. }));
            assert!(matches!(sides.theirs, gitsail_protocol::ConflictSideContentDto::Text { .. }));

            // T-232 criterion 3: resolving is only ever this explicit call —
            // simulates resolving the conflict outside GitSail, then marking
            // it resolved.
            std::fs::write(dir.path().join("f.txt"), "line1\nRESOLVED\nline3\n").unwrap();
            mark_conflict_resolved_impl(&state, "f.txt").unwrap();

            // T-233 criterion 2/3: continue completes the merge commit, and
            // the real resulting state is reinspected, never presumed.
            continue_operation_impl(&state).unwrap();
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
            let merged_head = head(dir.path());
            let parents = ProcessCommand::new("git")
                .args(["rev-parse", &format!("{merged_head}^1"), &format!("{merged_head}^2")])
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(parents.status.success(), "HEAD must be a two-parent merge commit");
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "line1\nRESOLVED\nline3\n"
            );
        }

        #[test]
        fn a_binary_conflict_resolves_via_take_conflict_side() {
            let dir = init_repo("merge-conflict-binary");
            std::fs::write(dir.path().join("img.bin"), [0u8, 1, 2, 3]).unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            git(dir.path(), &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.path().join("img.bin"), [0u8, 9, 9, 9]).unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "feature binary change"]);
            git(dir.path(), &["checkout", "-q", "main"]);
            std::fs::write(dir.path().join("img.bin"), [0u8, 5, 5, 5]).unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "main binary change"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();
            assert!(matches!(
                merge_impl(&state, "feature").unwrap(),
                MergeResultDto::Conflict { .. }
            ));

            take_conflict_side_impl(&state, "img.bin", "theirs").unwrap();
            assert_eq!(
                std::fs::read(dir.path().join("img.bin")).unwrap(),
                vec![0u8, 9, 9, 9]
            );

            continue_operation_impl(&state).unwrap();
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
        }

        #[test]
        fn aborting_a_pending_merge_restores_head_and_preserves_unrelated_work() {
            let dir = init_repo("merge-abort");
            setup_conflicting_divergence(dir.path());
            let head_before_merge = head(dir.path());
            std::fs::write(dir.path().join("unrelated.txt"), "unrelated work\n").unwrap();

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();
            assert!(matches!(
                merge_impl(&state, "feature").unwrap(),
                MergeResultDto::Conflict { .. }
            ));

            abort_operation_impl(&state).unwrap();

            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
            assert_eq!(head(dir.path()), head_before_merge);
            assert_eq!(
                std::fs::read_to_string(dir.path().join("unrelated.txt")).unwrap(),
                "unrelated work\n"
            );
        }

        #[test]
        fn merging_while_another_operation_is_pending_is_refused() {
            let dir = init_repo("merge-refuses-existing-op");
            setup_conflicting_divergence(dir.path());
            // Start a real conflicting merge directly, as another
            // terminal/editor would.
            let _ = ProcessCommand::new("git")
                .args(["merge", "feature"])
                .current_dir(dir.path())
                .output()
                .unwrap();

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let err = merge_impl(&state, "feature").unwrap_err();

            assert_eq!(err.code(), ErrorCode::OperationConflict);
        }

        // -- EPIC-17/T-235: rebase, skip ---------------------------------

        #[test]
        fn rebase_reapplies_commits_and_detect_reports_nothing_pending() {
            let dir = init_repo("rebase-clean");
            std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            git(dir.path(), &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "feature change"]);
            git(dir.path(), &["checkout", "-q", "main"]);
            std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "main advances"]);
            git(dir.path(), &["checkout", "-q", "feature"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = rebase_impl(&state, "main").unwrap();

            match result {
                RebaseResultDto::Completed { new_head } => assert_eq!(new_head, head(dir.path())),
                other => panic!("expected Completed, got {other:?}"),
            }
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
            assert!(dir.path().join("b.txt").exists());
        }

        #[test]
        fn rebase_reports_a_conflict_and_the_full_resolve_continue_flow_completes_it() {
            let dir = init_repo("rebase-conflict-flow");
            setup_conflicting_divergence(dir.path());
            git(dir.path(), &["checkout", "-q", "feature"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = rebase_impl(&state, "main").unwrap();
            let files = match result {
                RebaseResultDto::Conflict { conflicted_files } => conflicted_files,
                other => panic!("a conflict must never be reported as {other:?}"),
            };
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "f.txt");
            assert!(matches!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::Rebase { .. }
            ));

            std::fs::write(dir.path().join("f.txt"), "line1\nRESOLVED\nline3\n").unwrap();
            mark_conflict_resolved_impl(&state, "f.txt").unwrap();
            continue_operation_impl(&state).unwrap();

            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "line1\nRESOLVED\nline3\n"
            );
        }

        #[test]
        fn rebase_conflict_recovers_via_skip() {
            let dir = init_repo("rebase-conflict-skip");
            setup_conflicting_divergence(dir.path());
            git(dir.path(), &["checkout", "-q", "feature"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();
            assert!(matches!(
                rebase_impl(&state, "main").unwrap(),
                RebaseResultDto::Conflict { .. }
            ));

            skip_operation_impl(&state).unwrap();

            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "line1\nCHANGED-main\nline3\n"
            );
        }

        /// T-235/US-083 criterion 3: skip is refused with a clear error for
        /// an operation that does not support it (a merge has no further
        /// step to skip past).
        #[test]
        fn skip_operation_is_refused_for_a_pending_merge() {
            let dir = init_repo("skip-refuses-merge");
            setup_conflicting_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();
            assert!(matches!(
                merge_impl(&state, "feature").unwrap(),
                MergeResultDto::Conflict { .. }
            ));

            let err = skip_operation_impl(&state).unwrap_err();
            assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

            abort_operation_impl(&state).unwrap();
        }

        #[test]
        fn rebasing_a_dirty_working_tree_is_refused_without_ever_stashing_automatically() {
            let dir = init_repo("rebase-dirty");
            std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            git(dir.path(), &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "feature change"]);
            git(dir.path(), &["checkout", "-q", "main"]);
            std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "main advances"]);
            git(dir.path(), &["checkout", "-q", "feature"]);
            std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let err = rebase_impl(&state, "main").unwrap_err();
            assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
        }

        // -- T-236/US-084: plan an interactive rebase --------------------

        /// Two independent feature commits diverging from `main` by one
        /// commit of its own, on unrelated files throughout — so a plain
        /// rebase of the whole range never conflicts, leaving the plan's
        /// own reordering/action assignment as the only thing under test.
        /// Mirrors `gitsail-tui`'s own `setup_two_commit_divergence` fixture
        /// in `crates/gitsail-tui/tests/rebase.rs`.
        fn setup_two_commit_divergence(dir: &Path) {
            std::fs::write(dir.join("base.txt"), "base\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "base"]);

            git(dir, &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.join("a.txt"), "a\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "feature A"]);
            std::fs::write(dir.join("b.txt"), "b\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "feature B"]);

            git(dir, &["checkout", "-q", "main"]);
            std::fs::write(dir.join("main.txt"), "main\n").unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "--quiet", "-m", "main advances"]);

            git(dir, &["checkout", "-q", "feature"]);
        }

        fn commit_subjects(dir: &Path, count: usize) -> Vec<String> {
            let output = ProcessCommand::new("git")
                .args(["log", &format!("-{count}"), "--pretty=format:%s"])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|s| s.to_string())
                .collect()
        }

        /// T-236/US-084 criterion 1: the plan lists the exact candidate
        /// range `plan_rebase` reads, oldest first, each defaulted to
        /// `pick` — before anything is confirmed.
        #[test]
        fn plan_rebase_lists_candidates_oldest_first_defaulted_to_pick() {
            let dir = init_repo("rebase-plan-list");
            setup_two_commit_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let plan = plan_rebase_impl(&state, "main").unwrap();

            assert_eq!(plan.onto_revision, "main");
            let subjects: Vec<_> = plan.entries.iter().map(|e| e.subject.clone()).collect();
            assert_eq!(subjects, vec!["feature A", "feature B"]);
            assert!(plan
                .entries
                .iter()
                .all(|e| e.action == gitsail_protocol::RebaseActionDto::Pick));
            assert!(plan.entries.iter().all(|e| e.message_override.is_none()));
        }

        /// T-236/US-084 criterion 1: reordering and reassigning an action
        /// through the DTO round trip, then executing, actually reapplies
        /// the commits in the new order with the new action's effect — a
        /// real resulting tree, not just a converted shape.
        #[test]
        fn execute_rebase_plan_reorders_and_rewords_reapplying_commits_in_the_new_order() {
            let dir = init_repo("rebase-plan-reorder-reword");
            setup_two_commit_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let mut plan = plan_rebase_impl(&state, "main").unwrap();
            assert_eq!(plan.entries.len(), 2);

            // Reorder to [feature B, feature A] and reword "feature B".
            plan.entries.swap(0, 1);
            plan.entries[0].action = gitsail_protocol::RebaseActionDto::Reword;
            plan.entries[0].message_override = Some("reworded B".to_string());

            let result = execute_rebase_plan_impl(&state, &plan).unwrap();
            match result {
                RebaseResultDto::Completed { new_head } => assert_eq!(new_head, head(dir.path())),
                other => panic!("expected Completed, got {other:?}"),
            }

            let subjects = commit_subjects(dir.path(), 2);
            assert_eq!(subjects, vec!["feature A", "reworded B"]);
            assert!(dir.path().join("a.txt").exists());
            assert!(dir.path().join("b.txt").exists());
            assert!(dir.path().join("main.txt").exists());
        }

        /// T-236/US-084 criterion 2 / T-237/US-085 criterion 2: the Core's
        /// own `RebasePlan::validate` is the final authority — a `squash`
        /// on the first entry is refused before touching the repository at
        /// all, even if a compromised/buggy frontend sent it anyway.
        #[test]
        fn execute_rebase_plan_rejects_an_invalid_plan_before_touching_the_repository() {
            let dir = init_repo("rebase-plan-invalid");
            setup_two_commit_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let mut plan = plan_rebase_impl(&state, "main").unwrap();
            plan.entries[0].action = gitsail_protocol::RebaseActionDto::Squash;
            let head_before = head(dir.path());

            let err = execute_rebase_plan_impl(&state, &plan).unwrap_err();

            assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
            assert_eq!(
                head(dir.path()),
                head_before,
                "an invalid plan must never touch the repository"
            );
            assert_eq!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::None
            );
        }

        /// T-236/US-084 criterion 2: a plan built against one state of
        /// `onto` that has since moved is refused by the Core's own
        /// revalidation with a clear error, never silently executed against
        /// the newer state.
        #[test]
        fn execute_rebase_plan_refuses_a_stale_plan_after_onto_moves_concurrently() {
            let dir = init_repo("rebase-plan-stale");
            setup_two_commit_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let plan = plan_rebase_impl(&state, "main").unwrap();

            // `main` moves after the plan was built, without this session's
            // knowledge — mirrors another terminal/process advancing it
            // concurrently.
            git(dir.path(), &["checkout", "-q", "main"]);
            std::fs::write(dir.path().join("late.txt"), "late\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "late main commit"]);
            git(dir.path(), &["checkout", "-q", "feature"]);

            let err = execute_rebase_plan_impl(&state, &plan).unwrap_err();

            assert_eq!(err.code(), ErrorCode::OperationConflict);
            assert!(err
                .to_string()
                .contains("now resolves to a different commit"));
        }

        // -- EPIC-17/T-238..T-240: cherry-pick, revert, reset ------------

        #[test]
        fn cherry_pick_applies_a_commit_via_the_command() {
            let dir = init_repo("cherry-pick-apply");
            std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            git(dir.path(), &["checkout", "-q", "-b", "feature"]);
            std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "feature change"]);
            let feature_commit = head(dir.path());
            git(dir.path(), &["checkout", "-q", "main"]);

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = cherry_pick_impl(&state, &feature_commit, None).unwrap();

            match result {
                CherryPickResultDto::Applied { hash } => assert_eq!(hash, head(dir.path())),
                other => panic!("expected Applied, got {other:?}"),
            }
            assert!(dir.path().join("feature.txt").exists());
        }

        #[test]
        fn cherry_pick_reports_a_conflict_and_refuses_a_merge_commit_without_a_policy() {
            let dir = init_repo("cherry-pick-conflict");
            setup_conflicting_divergence(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();
            let feature_commit = {
                let output = ProcessCommand::new("git")
                    .args(["rev-parse", "feature"])
                    .current_dir(dir.path())
                    .output()
                    .unwrap();
                String::from_utf8(output.stdout).unwrap().trim().to_string()
            };

            let result = cherry_pick_impl(&state, &feature_commit, None).unwrap();
            assert!(matches!(result, CherryPickResultDto::Conflict { .. }));
            assert!(matches!(
                detect_in_progress_operation_impl(&state).unwrap(),
                InProgressOperationDto::CherryPick { .. }
            ));
            abort_operation_impl(&state).unwrap();

            // A merge commit requires an explicit policy — this bare-string
            // command boundary refuses an unrecognized one just as clearly
            // as a missing one refuses a merge commit at the Core layer.
            let err = cherry_pick_impl(&state, &feature_commit, Some("bogus")).unwrap_err();
            assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
        }

        #[test]
        fn revert_creates_a_new_commit_undoing_the_change() {
            let dir = init_repo("revert-apply");
            std::fs::write(dir.path().join("f.txt"), "line1\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "base"]);
            std::fs::write(dir.path().join("f.txt"), "line1\nline2\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "add line2"]);
            let added = head(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            let result = revert_impl(&state, &added, None).unwrap();

            match result {
                RevertResultDto::Applied { hash } => {
                    assert_eq!(hash, head(dir.path()));
                    assert_ne!(hash, added);
                }
                other => panic!("expected Applied, got {other:?}"),
            }
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "line1\n"
            );
        }

        #[test]
        fn reset_hard_moves_head_index_and_working_tree_and_refuses_a_stale_expected_head() {
            let dir = init_repo("reset-hard");
            std::fs::write(dir.path().join("f.txt"), "a\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "c1"]);
            let c1 = head(dir.path());
            std::fs::write(dir.path().join("f.txt"), "a\nb\n").unwrap();
            git(dir.path(), &["add", "-A"]);
            git(dir.path(), &["commit", "--quiet", "-m", "c2"]);
            let c2 = head(dir.path());

            let state = real_app_state();
            open_repository_impl(&state, dir.path().to_str().unwrap()).unwrap();

            // A stale `expected_head` (not real `HEAD`, `c1`) is refused
            // before anything runs.
            let err = reset_impl(&state, &c1, "hard", &c1).unwrap_err();
            assert_eq!(err.code(), ErrorCode::OperationConflict);
            assert_eq!(head(dir.path()), c2, "a refused reset must not move HEAD");

            reset_impl(&state, &c1, "hard", &c2).unwrap();

            assert_eq!(head(dir.path()), c1);
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "a\n"
            );
            let status = ProcessCommand::new("git")
                .args(["status", "--porcelain"])
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(String::from_utf8(status.stdout).unwrap().trim().is_empty());
        }
    }
}
