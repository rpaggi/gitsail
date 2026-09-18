//! Versioned data-transfer objects mapped explicitly from domain models
//! (SAD §14; ADR-008; US-035).
//!
//! Every DTO here is built through a `From<&domain type>` impl rather than
//! deriving `Serialize` on the domain type itself: the wire shape is this
//! crate's own contract, free to diverge from internal field layout, and a
//! domain-only change (renaming a field, adding a variant) can never
//! silently change the protocol (SAD §14: "Domain structs are not
//! serialized directly by default").

use std::path::Path;

use serde::{Deserialize, Serialize};

use gitsail_application::{AmendPreview, CommitDiff, PatchExport, PullOutcome, RecentRepositoryEntry};
use gitsail_domain::{
    Blame, BlameLine, BlameOrigin, Branch, BranchKind, ChangeType, Commit, CommitHash, Decoration,
    Diff, DiffHunk, DiffLine, DiffLineOrigin, FileChange, FileContentAtRevision, FileContentKind,
    FileDiff, FileStatusCode, GitTimestamp, GraphEdge, GraphRow, HeadState, LineHistory,
    LineHistoryEntry, LineRange, Remote, Repository, RepositoryStatus, Signature,
};

/// Converts a filesystem path to its wire representation.
///
/// Uses [`Path::to_string_lossy`], the same lossy conversion
/// [`gitsail_domain::RepositoryId`] already applies for its own display
/// identity: a path containing non-UTF-8 bytes (possible, if rare, on Unix)
/// has those bytes substituted rather than rejected. This is a known,
/// explicitly documented limitation of the current protocol version (SAD
/// §14) rather than a silent one: a future schema version may need a
/// lossless representation for non-UTF-8 paths.
fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The reverse of [`path_to_string`]: builds a [`std::path::PathBuf`] from
/// the wire representation. Used only where a DTO travels *back* into
/// domain shape (US-058/US-191's hunk-selection payload) — every other DTO
/// in this module is one-directional (domain -> wire only), per this
/// module's own doc comment.
fn string_to_path(value: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(value)
}

// ---------------------------------------------------------------------
// Repository / HEAD state.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HeadStateDto {
    Attached { branch: String },
    Detached { commit: String },
    Unborn,
}

impl From<&HeadState> for HeadStateDto {
    fn from(state: &HeadState) -> Self {
        match state {
            HeadState::Attached { branch } => Self::Attached {
                branch: branch.as_str().to_string(),
            },
            HeadState::Detached { commit } => Self::Detached {
                commit: commit.as_str().to_string(),
            },
            HeadState::Unborn => Self::Unborn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDto {
    pub id: String,
    pub root_path: String,
    pub worktree_path: Option<String>,
    pub is_bare: bool,
    pub head_state: HeadStateDto,
    pub current_branch: Option<String>,
}

impl From<&Repository> for RepositoryDto {
    fn from(repo: &Repository) -> Self {
        Self {
            id: repo.id.as_str().to_string(),
            root_path: path_to_string(&repo.root_path),
            worktree_path: repo.worktree_path.as_deref().map(path_to_string),
            is_bare: repo.is_bare,
            head_state: HeadStateDto::from(&repo.head_state),
            current_branch: repo.current_branch.as_ref().map(|b| b.as_str().to_string()),
        }
    }
}

// ---------------------------------------------------------------------
// Status.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeTypeDto {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
    Ignored,
}

impl From<ChangeType> for ChangeTypeDto {
    fn from(value: ChangeType) -> Self {
        match value {
            ChangeType::Added => Self::Added,
            ChangeType::Modified => Self::Modified,
            ChangeType::Deleted => Self::Deleted,
            ChangeType::Renamed => Self::Renamed,
            ChangeType::Copied => Self::Copied,
            ChangeType::TypeChanged => Self::TypeChanged,
            ChangeType::Unmerged => Self::Unmerged,
            ChangeType::Untracked => Self::Untracked,
            ChangeType::Ignored => Self::Ignored,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatusCodeDto {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    UpdatedButUnmerged,
    Untracked,
    Ignored,
}

impl From<FileStatusCode> for FileStatusCodeDto {
    fn from(value: FileStatusCode) -> Self {
        match value {
            FileStatusCode::Unmodified => Self::Unmodified,
            FileStatusCode::Modified => Self::Modified,
            FileStatusCode::Added => Self::Added,
            FileStatusCode::Deleted => Self::Deleted,
            FileStatusCode::Renamed => Self::Renamed,
            FileStatusCode::Copied => Self::Copied,
            FileStatusCode::UpdatedButUnmerged => Self::UpdatedButUnmerged,
            FileStatusCode::Untracked => Self::Untracked,
            FileStatusCode::Ignored => Self::Ignored,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeDto {
    pub path: String,
    pub previous_path: Option<String>,
    pub change_type: ChangeTypeDto,
    pub index_status: FileStatusCodeDto,
    pub worktree_status: FileStatusCodeDto,
}

impl From<&FileChange> for FileChangeDto {
    fn from(change: &FileChange) -> Self {
        Self {
            path: path_to_string(&change.path),
            previous_path: change.previous_path.as_deref().map(path_to_string),
            change_type: change.change_type.into(),
            index_status: change.index_status.into(),
            worktree_status: change.worktree_status.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatusDto {
    pub branch: Option<String>,
    pub head_state: HeadStateDto,
    pub files: Vec<FileChangeDto>,
    pub is_clean: bool,
}

impl From<&RepositoryStatus> for RepositoryStatusDto {
    fn from(status: &RepositoryStatus) -> Self {
        Self {
            branch: status.branch.as_ref().map(|b| b.as_str().to_string()),
            head_state: HeadStateDto::from(&status.head_state),
            files: status.files.iter().map(FileChangeDto::from).collect(),
            is_clean: status.is_clean(),
        }
    }
}

// ---------------------------------------------------------------------
// Commits.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTimestampDto {
    pub seconds_since_epoch: i64,
    pub utc_offset_minutes: i32,
}

impl From<GitTimestamp> for GitTimestampDto {
    fn from(ts: GitTimestamp) -> Self {
        Self {
            seconds_since_epoch: ts.seconds_since_epoch,
            utc_offset_minutes: ts.utc_offset_minutes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureDto {
    pub name: String,
    pub email: String,
}

impl From<&Signature> for SignatureDto {
    fn from(sig: &Signature) -> Self {
        Self {
            name: sig.name.clone(),
            email: sig.email.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DecorationDto {
    Head,
    Branch { name: String },
    RemoteBranch { remote: String, branch: String },
    Tag { name: String },
}

impl From<&Decoration> for DecorationDto {
    fn from(decoration: &Decoration) -> Self {
        match decoration {
            Decoration::Head => Self::Head,
            Decoration::Branch(name) => Self::Branch {
                name: name.as_str().to_string(),
            },
            Decoration::RemoteBranch { remote, branch } => Self::RemoteBranch {
                remote: remote.clone(),
                branch: branch.as_str().to_string(),
            },
            Decoration::Tag(name) => Self::Tag { name: name.clone() },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDto {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: SignatureDto,
    pub committer: SignatureDto,
    pub author_date: GitTimestampDto,
    pub commit_date: GitTimestampDto,
    pub subject: String,
    pub body: String,
    pub decorations: Vec<DecorationDto>,
    pub is_merge: bool,
    pub is_root: bool,
}

impl From<&Commit> for CommitDto {
    fn from(commit: &Commit) -> Self {
        Self {
            hash: commit.hash.as_str().to_string(),
            short_hash: commit.short_hash.as_str().to_string(),
            parents: commit.parents.iter().map(|p| p.as_str().to_string()).collect(),
            author: SignatureDto::from(&commit.author),
            committer: SignatureDto::from(&commit.committer),
            author_date: commit.author_date.into(),
            commit_date: commit.commit_date.into(),
            subject: commit.subject.clone(),
            body: commit.body.clone(),
            decorations: commit.decorations.iter().map(DecorationDto::from).collect(),
            is_merge: commit.is_merge(),
            is_root: commit.is_root(),
        }
    }
}

// ---------------------------------------------------------------------
// Commit graph (US-067): the Core-computed lanes/edges from
// `gitsail_domain::graph`, sent as-is so the Desktop frontend never
// recomputes a layout itself (US-067 criterion 3) — it only ever renders
// what this crate serializes here.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeDto {
    pub from_lane: u32,
    pub to_lane: u32,
    pub target: String,
    pub resolved: bool,
}

impl From<&GraphEdge> for GraphEdgeDto {
    fn from(edge: &GraphEdge) -> Self {
        Self {
            from_lane: edge.from_lane as u32,
            to_lane: edge.to_lane as u32,
            target: edge.target.as_str().to_string(),
            resolved: edge.resolved,
        }
    }
}

/// One rendered row: the [`CommitDto`] it represents plus the lane/edge
/// data [`GraphRow`] carries. Built by zipping a [`GraphRow`] with the
/// [`Commit`] at the same index — see
/// [`gitsail_domain::graph`]'s documentation for why that pairing is
/// always safe (`CommitGraph::append_page` emits exactly one row per input
/// commit, in order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitGraphRowDto {
    pub commit: CommitDto,
    pub lane: u32,
    pub edges: Vec<GraphEdgeDto>,
    pub passthrough_lanes: Vec<u32>,
}

impl CommitGraphRowDto {
    pub fn from_row_and_commit(row: &GraphRow, commit: &Commit) -> Self {
        Self {
            commit: CommitDto::from(commit),
            lane: row.lane as u32,
            edges: row.edges.iter().map(GraphEdgeDto::from).collect(),
            passthrough_lanes: row.passthrough_lanes.iter().map(|l| *l as u32).collect(),
        }
    }
}

/// One page of commit-graph rows plus continuation metadata (SAD §14, §25),
/// mirroring [`crate::envelope::Page`] but adding `lane_count` — the widest
/// lane column a renderer needs to reserve across every row accumulated so
/// far, not just this page's own rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitGraphPageDto {
    pub rows: Vec<CommitGraphRowDto>,
    pub lane_count: u32,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------
// Recent repositories (US-052). Mirrors
// `gitsail_application::RecentRepositoryEntry` — the generic list logic
// lives there (reusable by any future frontend), this crate only adds the
// wire shape.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRepositoryDto {
    pub path: String,
    pub last_opened_unix_seconds: i64,
}

impl From<&RecentRepositoryEntry> for RecentRepositoryDto {
    fn from(entry: &RecentRepositoryEntry) -> Self {
        Self {
            path: path_to_string(&entry.path),
            last_opened_unix_seconds: entry.last_opened_unix_seconds,
        }
    }
}

// ---------------------------------------------------------------------
// Branches.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BranchKindDto {
    Local,
    Remote { remote: String },
}

impl From<&BranchKind> for BranchKindDto {
    fn from(kind: &BranchKind) -> Self {
        match kind {
            BranchKind::Local => Self::Local,
            BranchKind::Remote { remote } => Self::Remote {
                remote: remote.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDto {
    pub name: String,
    pub kind: BranchKindDto,
    pub target: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub is_current: bool,
}

impl From<&Branch> for BranchDto {
    fn from(branch: &Branch) -> Self {
        Self {
            name: branch.name.as_str().to_string(),
            kind: BranchKindDto::from(&branch.kind),
            target: branch.target.as_str().to_string(),
            upstream: branch.upstream.as_ref().map(|u| u.as_str().to_string()),
            ahead: branch.ahead,
            behind: branch.behind,
            is_current: branch.is_current,
        }
    }
}

// ---------------------------------------------------------------------
// Remotes and sync (EPIC-19/US-096..098; exposed to Desktop for US-060/
// T-193's fetch/pull/push subset). Mirrors `gitsail-tui`'s own rendering
// convention (`crates/gitsail-tui/src/ui.rs`): a remote's URLs always cross
// this boundary already redacted (`RemoteUrl::redacted`), never the raw
// string — the same "never leak an embedded credential" rule SAD §11/§28
// hold for any diagnostic surface applies here too.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDto {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

impl From<&Remote> for RemoteDto {
    fn from(remote: &Remote) -> Self {
        Self {
            name: remote.name.clone(),
            fetch_url: remote.fetch_url.redacted(),
            push_url: remote.push_url.redacted(),
        }
    }
}

/// Which remote (and, for pull/push, which branch) a sync action would
/// target — returned by `resolve_sync_target` for a caller to display
/// *before* running fetch/pull/push (US-060 criterion 2), and echoed back
/// by `fetch`/`push` themselves once they have run, naming exactly what
/// they acted on. `branch` is the current branch used to resolve the
/// remote (via its upstream, when set) — always present once a repository
/// with a checked-out branch is open, fetch included, since knowing which
/// branch informed the choice is part of "never an implicit, silently
/// guessed choice" (mirrors `gitsail_tui::App::resolve_sync_remote`'s own
/// contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTargetDto {
    pub remote: String,
    pub branch: Option<String>,
}

/// A pull's exact outcome (US-097/US-060 criterion 2: "already up to date"
/// and "fast-forwarded" are shown explicitly, never collapsed into a bare
/// success) — mirrors `gitsail_application::PullOutcome` one-to-one, the
/// same as `gitsail-tui`'s own rendering of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum PullOutcomeDto {
    AlreadyUpToDate,
    // `rename_all` on an enum only renames variant names, not struct-variant
    // field names (no existing DTO here had an underscored field inside an
    // enum variant to reveal that until this one) — spelled out explicitly
    // so the wire field is `newHead`, matching every other DTO's camelCase
    // convention.
    FastForwarded {
        #[serde(rename = "newHead")]
        new_head: String,
    },
}

impl From<&PullOutcome> for PullOutcomeDto {
    fn from(outcome: &PullOutcome) -> Self {
        match outcome {
            PullOutcome::AlreadyUpToDate => Self::AlreadyUpToDate,
            PullOutcome::FastForwarded { new_head } => Self::FastForwarded {
                new_head: new_head.as_str().to_string(),
            },
        }
    }
}

/// A completed pull's full report: which remote/branch it targeted plus its
/// [`PullOutcomeDto`] (US-060 criterion 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResultDto {
    pub remote: String,
    pub branch: String,
    pub outcome: PullOutcomeDto,
}

// ---------------------------------------------------------------------
// Diffs.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineOriginDto {
    Context,
    Addition,
    Deletion,
}

impl From<DiffLineOrigin> for DiffLineOriginDto {
    fn from(origin: DiffLineOrigin) -> Self {
        match origin {
            DiffLineOrigin::Context => Self::Context,
            DiffLineOrigin::Addition => Self::Addition,
            DiffLineOrigin::Deletion => Self::Deletion,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLineDto {
    pub origin: DiffLineOriginDto,
    pub content: String,
    pub has_trailing_newline: bool,
}

impl From<&DiffLine> for DiffLineDto {
    fn from(line: &DiffLine) -> Self {
        Self {
            origin: line.origin.into(),
            content: line.content.clone(),
            has_trailing_newline: line.has_trailing_newline,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunkDto {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineDto>,
}

impl From<&DiffHunk> for DiffHunkDto {
    fn from(hunk: &DiffHunk) -> Self {
        Self {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            lines: hunk.lines.iter().map(DiffLineDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffDto {
    pub path: String,
    pub previous_path: Option<String>,
    pub change_type: ChangeTypeDto,
    pub is_binary: bool,
    pub truncated: bool,
    pub hunks: Vec<DiffHunkDto>,
}

impl From<&FileDiff> for FileDiffDto {
    fn from(file: &FileDiff) -> Self {
        Self {
            path: path_to_string(&file.path),
            previous_path: file.previous_path.as_deref().map(path_to_string),
            change_type: file.change_type.into(),
            is_binary: file.is_binary,
            truncated: file.truncated,
            hunks: file.hunks.iter().map(DiffHunkDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffDto {
    pub files: Vec<FileDiffDto>,
}

impl From<&Diff> for DiffDto {
    fn from(diff: &Diff) -> Self {
        Self {
            files: diff.files.iter().map(FileDiffDto::from).collect(),
        }
    }
}

// -- Reverse direction: hunk-selection payloads (US-058/T-191) ----------
//
// Every DTO above only ever travels domain -> wire. A hunk-level
// stage/unstage selection is the one payload that must travel the other
// way (the frontend echoes back exactly the hunks it read from a prior
// `DiffDto`, trimmed to the ones the person selected) into
// `RepositoryWritePort::stage_hunks`/`unstage_hunks`, which takes
// `&[FileDiff]` — domain shape, not DTOs. These conversions are
// infallible: every DTO variant maps onto exactly one domain variant, so
// there is no "wire value with no domain meaning" to reject.

impl From<DiffLineOriginDto> for DiffLineOrigin {
    fn from(value: DiffLineOriginDto) -> Self {
        match value {
            DiffLineOriginDto::Context => Self::Context,
            DiffLineOriginDto::Addition => Self::Addition,
            DiffLineOriginDto::Deletion => Self::Deletion,
        }
    }
}

impl From<ChangeTypeDto> for ChangeType {
    fn from(value: ChangeTypeDto) -> Self {
        match value {
            ChangeTypeDto::Added => Self::Added,
            ChangeTypeDto::Modified => Self::Modified,
            ChangeTypeDto::Deleted => Self::Deleted,
            ChangeTypeDto::Renamed => Self::Renamed,
            ChangeTypeDto::Copied => Self::Copied,
            ChangeTypeDto::TypeChanged => Self::TypeChanged,
            ChangeTypeDto::Unmerged => Self::Unmerged,
            ChangeTypeDto::Untracked => Self::Untracked,
            ChangeTypeDto::Ignored => Self::Ignored,
        }
    }
}

impl From<&DiffLineDto> for DiffLine {
    fn from(line: &DiffLineDto) -> Self {
        Self {
            origin: line.origin.into(),
            content: line.content.clone(),
            has_trailing_newline: line.has_trailing_newline,
        }
    }
}

impl From<&DiffHunkDto> for DiffHunk {
    fn from(hunk: &DiffHunkDto) -> Self {
        Self {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            lines: hunk.lines.iter().map(DiffLine::from).collect(),
        }
    }
}

impl From<&FileDiffDto> for FileDiff {
    fn from(file: &FileDiffDto) -> Self {
        Self {
            path: string_to_path(&file.path),
            previous_path: file.previous_path.as_deref().map(string_to_path),
            change_type: file.change_type.into(),
            is_binary: file.is_binary,
            truncated: file.truncated,
            hunks: file.hunks.iter().map(DiffHunk::from).collect(),
        }
    }
}

// ---------------------------------------------------------------------
// Patch export (US-029/T-162). Mirrors
// `gitsail_application::patch::PatchExport` — the rendering and scope
// classification logic lives there (shared by the TUI), this crate only
// adds the wire shape so the Desktop frontend can copy/save it without
// ever parsing or reconstructing a patch itself.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchExportDto {
    pub patch: String,
    pub included_files: Vec<String>,
    pub skipped_binary_files: Vec<String>,
    pub skipped_truncated_files: Vec<String>,
}

impl From<&PatchExport> for PatchExportDto {
    fn from(export: &PatchExport) -> Self {
        Self {
            patch: export.patch.clone(),
            included_files: export.included_files.iter().map(|p| path_to_string(p)).collect(),
            skipped_binary_files: export
                .skipped_binary_files
                .iter()
                .map(|p| path_to_string(p))
                .collect(),
            skipped_truncated_files: export
                .skipped_truncated_files
                .iter()
                .map(|p| path_to_string(p))
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------
// Blame.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlameOriginDto {
    Committed,
    Local,
}

impl From<BlameOrigin> for BlameOriginDto {
    fn from(origin: BlameOrigin) -> Self {
        match origin {
            BlameOrigin::Committed => Self::Committed,
            BlameOrigin::Local => Self::Local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLineDto {
    pub final_line: u32,
    pub original_line: u32,
    pub commit: String,
    pub author: SignatureDto,
    pub timestamp: GitTimestampDto,
    pub content: String,
    pub origin: BlameOriginDto,
}

impl From<&BlameLine> for BlameLineDto {
    fn from(line: &BlameLine) -> Self {
        Self {
            final_line: line.final_line,
            original_line: line.original_line,
            commit: line.commit.as_str().to_string(),
            author: SignatureDto::from(&line.author),
            timestamp: line.timestamp.into(),
            content: line.content.clone(),
            origin: line.origin.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameDto {
    pub file: String,
    pub revision: Option<String>,
    pub lines: Vec<BlameLineDto>,
}

impl From<&Blame> for BlameDto {
    fn from(blame: &Blame) -> Self {
        Self {
            file: path_to_string(&blame.file),
            revision: blame.revision.as_ref().map(|r| r.as_str().to_string()),
            lines: blame.lines.iter().map(BlameLineDto::from).collect(),
        }
    }
}

// ---------------------------------------------------------------------
// Single-commit diff (EPIC-15/US-076). Mirrors `gitsail_application::
// CommitDiff` — `base` is `None` for a root commit (diffed against the
// empty tree), naming exactly which commit was used otherwise.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDiffDto {
    pub target: String,
    pub base: Option<String>,
    pub diff: DiffDto,
}

impl From<&CommitDiff> for CommitDiffDto {
    fn from(value: &CommitDiff) -> Self {
        Self {
            target: value.target.as_str().to_string(),
            base: value.base.as_ref().map(|b| b.as_str().to_string()),
            diff: DiffDto::from(&value.diff),
        }
    }
}

// ---------------------------------------------------------------------
// Line/range history (US-019/EPIC-15).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRangeDto {
    pub start: u32,
    pub end: u32,
}

impl From<LineRange> for LineRangeDto {
    fn from(range: LineRange) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHistoryEntryDto {
    pub commit: CommitDto,
    pub hunks: Vec<DiffHunkDto>,
}

impl From<&LineHistoryEntry> for LineHistoryEntryDto {
    fn from(entry: &LineHistoryEntry) -> Self {
        Self {
            commit: CommitDto::from(&entry.commit),
            hunks: entry.hunks.iter().map(DiffHunkDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHistoryDto {
    pub file: String,
    pub revision: String,
    pub range: LineRangeDto,
    pub entries: Vec<LineHistoryEntryDto>,
}

impl From<&LineHistory> for LineHistoryDto {
    fn from(history: &LineHistory) -> Self {
        Self {
            file: path_to_string(&history.file),
            revision: history.revision.as_str().to_string(),
            range: LineRangeDto::from(history.range),
            entries: history.entries.iter().map(LineHistoryEntryDto::from).collect(),
        }
    }
}

// ---------------------------------------------------------------------
// File content at a revision (EPIC-15/US-076): a tagged enum so a caller
// can distinguish real text content from the two other legitimate,
// non-error outcomes (a binary file, or a path that did not exist at that
// revision) without inspecting a separate error channel.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileContentDto {
    Text {
        path: String,
        revision: String,
        content: String,
    },
    Binary {
        path: String,
        revision: String,
    },
    Missing {
        path: String,
        revision: String,
    },
}

impl From<&FileContentAtRevision> for FileContentDto {
    fn from(value: &FileContentAtRevision) -> Self {
        let path = path_to_string(&value.path);
        let revision = value.revision.as_str().to_string();
        match &value.kind {
            FileContentKind::Text(content) => Self::Text {
                path,
                revision,
                content: content.clone(),
            },
            FileContentKind::Binary => Self::Binary { path, revision },
            FileContentKind::Missing => Self::Missing { path, revision },
        }
    }
}

// ---------------------------------------------------------------------
// Commit/amend results (US-058/US-059). A single new commit hash, shared
// by `create_commit` and `amend_commit` — both mutate `HEAD` and the only
// thing either needs to report on success is the resulting commit's
// identity; the frontend already re-fetches status/graph itself
// afterward rather than this DTO trying to describe the mutation's full
// effect.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResultDto {
    pub hash: String,
}

impl From<&CommitHash> for CommitResultDto {
    fn from(hash: &CommitHash) -> Self {
        Self {
            hash: hash.as_str().to_string(),
        }
    }
}

// ---------------------------------------------------------------------
// Amend preview (US-059 criterion 1). Mirrors
// `gitsail_application::AmendPreview`: `HEAD`'s exact commit (for its
// current message/identity) and the staged diff that would be folded in.
// `head.hash` is what the frontend must echo back as `amend_commit`'s
// `expectedHead` — the same value naturally already sits in `head` here,
// so no separate field duplicates it.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmendPreviewDto {
    pub head: CommitDto,
    pub staged_diff: DiffDto,
}

impl From<&AmendPreview> for AmendPreviewDto {
    fn from(preview: &AmendPreview) -> Self {
        Self {
            head: CommitDto::from(&preview.head),
            staged_diff: DiffDto::from(&preview.staged_diff),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        BranchName, CommitHash, GitTimestamp, RepositoryId, ShortHash,
    };
    use std::path::PathBuf;

    #[test]
    fn repository_dto_never_serializes_a_debug_field_and_maps_head_state() {
        let repo = Repository {
            id: RepositoryId::from_canonical_root(Path::new("/repo")),
            root_path: PathBuf::from("/repo"),
            worktree_path: Some(PathBuf::from("/repo")),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        };

        let dto = RepositoryDto::from(&repo);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["rootPath"], "/repo");
        assert_eq!(json["headState"]["state"], "attached");
        assert_eq!(json["headState"]["branch"], "main");
        // The DTO's own field set is exhaustively asserted, so an
        // accidental extra (internal) field would fail this comparison.
        let keys: std::collections::HashSet<String> =
            json.as_object().unwrap().keys().cloned().collect();
        let expected: std::collections::HashSet<String> =
            ["id", "rootPath", "worktreePath", "isBare", "headState", "currentBranch"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn detached_and_unborn_head_states_map_distinctly() {
        let detached = HeadStateDto::from(&HeadState::Detached {
            commit: CommitHash::new("a".repeat(40)).unwrap(),
        });
        let unborn = HeadStateDto::from(&HeadState::Unborn);

        assert_eq!(serde_json::to_value(&detached).unwrap()["state"], "detached");
        assert_eq!(serde_json::to_value(&unborn).unwrap()["state"], "unborn");
    }

    #[test]
    fn commit_dto_surfaces_derived_root_and_merge_flags() {
        let commit = Commit {
            hash: CommitHash::new("a".repeat(40)).unwrap(),
            short_hash: ShortHash::new("aaaaaaaa").unwrap(),
            parents: vec![],
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: "root commit".into(),
            body: String::new(),
            decorations: vec![],
        };

        let dto = CommitDto::from(&commit);
        assert!(dto.is_root);
        assert!(!dto.is_merge);
    }

    #[test]
    fn commit_graph_row_dto_carries_lane_edges_and_the_zipped_commit() {
        let mut graph = gitsail_domain::CommitGraph::new();
        let commit = Commit {
            hash: CommitHash::new("a".repeat(40)).unwrap(),
            short_hash: ShortHash::new("aaaaaaaa").unwrap(),
            parents: vec![CommitHash::new("b".repeat(40)).unwrap()],
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: "add feature".into(),
            body: String::new(),
            decorations: vec![],
        };
        graph.append_page(&[gitsail_domain::GraphCommit::from(&commit)]);
        let row = &graph.rows()[0];

        let dto = CommitGraphRowDto::from_row_and_commit(row, &commit);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(dto.commit.hash, commit.hash.as_str());
        assert_eq!(dto.lane, 0);
        assert_eq!(dto.edges.len(), 1);
        assert!(
            !dto.edges[0].resolved,
            "a parent not present in this batch must be an unresolved continuation"
        );
        assert_eq!(json["edges"][0]["resolved"], false);
        assert_eq!(json["passthroughLanes"], serde_json::json!([]));
    }

    #[test]
    fn commit_graph_page_dto_omits_next_cursor_when_absent() {
        let page = CommitGraphPageDto {
            rows: vec![],
            lane_count: 0,
            has_more: false,
            next_cursor: None,
        };
        let json = serde_json::to_value(&page).unwrap();
        assert!(json.get("nextCursor").is_none());
        assert_eq!(json["hasMore"], false);
    }

    #[test]
    fn recent_repository_dto_maps_path_and_timestamp() {
        let entry = RecentRepositoryEntry {
            path: PathBuf::from("/repo"),
            last_opened_unix_seconds: 1_700_000_000,
        };

        let dto = RecentRepositoryDto::from(&entry);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(dto.path, "/repo");
        assert_eq!(json["lastOpenedUnixSeconds"], 1_700_000_000);
    }

    #[test]
    fn patch_export_dto_maps_patch_text_and_every_scope_bucket() {
        let export = PatchExport {
            patch: "--- a/a.txt\n+++ b/a.txt\n".to_string(),
            included_files: vec![PathBuf::from("a.txt")],
            skipped_binary_files: vec![PathBuf::from("image.png")],
            skipped_truncated_files: vec![PathBuf::from("huge.txt")],
        };

        let dto = PatchExportDto::from(&export);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(dto.patch, export.patch);
        assert_eq!(json["includedFiles"], serde_json::json!(["a.txt"]));
        assert_eq!(json["skippedBinaryFiles"], serde_json::json!(["image.png"]));
        assert_eq!(json["skippedTruncatedFiles"], serde_json::json!(["huge.txt"]));
    }

    #[test]
    fn blame_dto_echoes_the_queried_file_and_revision() {
        let blame = Blame {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            lines: vec![],
        };
        let dto = BlameDto::from(&blame);
        assert_eq!(dto.file, "src/lib.rs");
        assert_eq!(dto.revision, None);
    }

    #[test]
    fn commit_diff_dto_reports_no_base_for_a_root_commit() {
        let commit_diff = CommitDiff {
            target: CommitHash::new("a".repeat(40)).unwrap(),
            base: None,
            diff: Diff { files: vec![] },
        };

        let dto = CommitDiffDto::from(&commit_diff);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["target"], "a".repeat(40));
        assert!(json["base"].is_null());
        assert_eq!(json["diff"]["files"], serde_json::json!([]));
    }

    #[test]
    fn line_history_dto_maps_range_and_entries() {
        let commit = Commit {
            hash: CommitHash::new("a".repeat(40)).unwrap(),
            short_hash: ShortHash::new("aaaaaaaa").unwrap(),
            parents: vec![],
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: "touch the range".into(),
            body: String::new(),
            decorations: vec![],
        };
        let history = LineHistory {
            file: PathBuf::from("src/lib.rs"),
            revision: CommitHash::new("b".repeat(40)).unwrap(),
            range: gitsail_domain::LineRange::new(10, 20),
            entries: vec![LineHistoryEntry {
                commit,
                hunks: vec![],
            }],
        };

        let dto = LineHistoryDto::from(&history);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["range"]["start"], 10);
        assert_eq!(json["range"]["end"], 20);
        assert_eq!(dto.entries.len(), 1);
        assert_eq!(json["entries"][0]["commit"]["subject"], "touch the range");
    }

    #[test]
    fn file_content_dto_tags_each_variant_distinctly() {
        let path = PathBuf::from("src/lib.rs");
        let revision = CommitHash::new("a".repeat(40)).unwrap();

        let text = FileContentDto::from(&FileContentAtRevision {
            path: path.clone(),
            revision: revision.clone(),
            kind: FileContentKind::Text("hello\n".into()),
        });
        let binary = FileContentDto::from(&FileContentAtRevision {
            path: path.clone(),
            revision: revision.clone(),
            kind: FileContentKind::Binary,
        });
        let missing = FileContentDto::from(&FileContentAtRevision {
            path,
            revision,
            kind: FileContentKind::Missing,
        });

        let text_json = serde_json::to_value(&text).unwrap();
        let binary_json = serde_json::to_value(&binary).unwrap();
        let missing_json = serde_json::to_value(&missing).unwrap();

        assert_eq!(text_json["kind"], "text");
        assert_eq!(text_json["content"], "hello\n");
        assert_eq!(binary_json["kind"], "binary");
        assert!(binary_json.get("content").is_none());
        assert_eq!(missing_json["kind"], "missing");
        assert!(missing_json.get("content").is_none());
    }

    #[test]
    fn commit_result_dto_serializes_the_hash_field() {
        let hash = CommitHash::new("a".repeat(40)).unwrap();

        let dto = CommitResultDto::from(&hash);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["hash"], "a".repeat(40));
    }

    #[test]
    fn amend_preview_dto_nests_the_head_commit_and_the_staged_diff() {
        let hash = CommitHash::new("b".repeat(40)).unwrap();
        let preview = AmendPreview {
            head: Commit {
                short_hash: hash.to_short(8),
                hash: hash.clone(),
                parents: vec![],
                author: Signature::new("Ada", "ada@example.com"),
                committer: Signature::new("Ada", "ada@example.com"),
                author_date: GitTimestamp::new(0, 0),
                commit_date: GitTimestamp::new(0, 0),
                subject: "original message".to_string(),
                body: String::new(),
                decorations: vec![],
            },
            staged_diff: Diff {
                files: vec![FileDiff {
                    path: PathBuf::from("a.txt"),
                    previous_path: None,
                    change_type: ChangeType::Modified,
                    is_binary: false,
                    truncated: false,
                    hunks: vec![],
                }],
            },
        };

        let dto = AmendPreviewDto::from(&preview);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["head"]["subject"], "original message");
        assert_eq!(json["head"]["hash"], "b".repeat(40));
        assert_eq!(json["stagedDiff"]["files"][0]["path"], "a.txt");
    }

    // -- Remotes and sync (EPIC-19/US-060/T-193) ---------------------------

    #[test]
    fn remote_dto_redacts_embedded_credentials_in_both_urls() {
        let remote = gitsail_domain::Remote {
            name: "origin".to_string(),
            fetch_url: gitsail_domain::RemoteUrl::new(
                "https://user:secret-token@github.com/org/repo.git",
            ),
            push_url: gitsail_domain::RemoteUrl::new(
                "https://user:secret-token@github.com/org/repo.git",
            ),
        };

        let dto = RemoteDto::from(&remote);

        assert_eq!(dto.name, "origin");
        assert!(!dto.fetch_url.contains("secret-token"));
        assert!(!dto.push_url.contains("secret-token"));
        assert_eq!(dto.fetch_url, "https://***@github.com/org/repo.git");
    }

    #[test]
    fn pull_outcome_dto_tags_already_up_to_date_and_fast_forwarded_distinctly() {
        let up_to_date = PullOutcomeDto::from(&PullOutcome::AlreadyUpToDate);
        let hash = CommitHash::new("a".repeat(40)).unwrap();
        let fast_forwarded = PullOutcomeDto::from(&PullOutcome::FastForwarded { new_head: hash.clone() });

        let up_to_date_json = serde_json::to_value(&up_to_date).unwrap();
        let fast_forwarded_json = serde_json::to_value(&fast_forwarded).unwrap();

        assert_eq!(up_to_date_json["outcome"], "alreadyUpToDate");
        assert_eq!(fast_forwarded_json["outcome"], "fastForwarded");
        assert_eq!(fast_forwarded_json["newHead"], "a".repeat(40));
    }

    #[test]
    fn pull_result_dto_carries_the_resolved_target_and_outcome() {
        let dto = PullResultDto {
            remote: "origin".to_string(),
            branch: "main".to_string(),
            outcome: PullOutcomeDto::AlreadyUpToDate,
        };

        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["remote"], "origin");
        assert_eq!(json["branch"], "main");
        assert_eq!(json["outcome"]["outcome"], "alreadyUpToDate");
    }

    #[test]
    fn sync_target_dto_allows_an_absent_branch() {
        let dto = SyncTargetDto {
            remote: "origin".to_string(),
            branch: None,
        };

        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["remote"], "origin");
        assert!(json["branch"].is_null());
    }

    #[test]
    fn file_diff_dto_round_trips_through_the_domain_type_for_a_hunk_selection() {
        let original = FileDiff {
            path: PathBuf::from("src/lib.rs"),
            previous_path: Some(PathBuf::from("src/old.rs")),
            change_type: ChangeType::Renamed,
            is_binary: false,
            truncated: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 3,
                lines: vec![
                    DiffLine {
                        origin: DiffLineOrigin::Context,
                        content: "unchanged".to_string(),
                        has_trailing_newline: true,
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Addition,
                        content: "new line".to_string(),
                        has_trailing_newline: true,
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Deletion,
                        content: "old line".to_string(),
                        has_trailing_newline: false,
                    },
                ],
            }],
        };

        let dto = FileDiffDto::from(&original);
        let round_tripped = FileDiff::from(&dto);

        assert_eq!(round_tripped, original, "a hunk-selection DTO must survive the trip back into domain shape unchanged");
    }
}
