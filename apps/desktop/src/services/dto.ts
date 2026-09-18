// Hand-written mirrors of the wire shapes `gitsail-protocol` defines in
// `crates/gitsail-protocol/src/dto.rs` (source of truth). These are shape
// mirrors only, not logic: no parsing or Git rule is reimplemented here
// (US-051 criterion 3), just field names/types matching the Rust DTOs'
// `#[serde(rename_all = "camelCase")]` wire format.
//
// Maintenance note: kept by hand rather than generated because this story
// only needs two DTOs. If a second consumer of `gitsail-protocol` (e.g. the
// VS Code extension) makes hand-maintaining two clients painful, revisit a
// codegen tool (e.g. ts-rs/typeshare) instead of adding a third hand-written
// copy.

export type HeadStateDto =
  | { state: "attached"; branch: string }
  | { state: "detached"; commit: string }
  | { state: "unborn" };

export interface RepositoryDto {
  id: string;
  rootPath: string;
  worktreePath: string | null;
  isBare: boolean;
  headState: HeadStateDto;
  currentBranch: string | null;
}

export type ChangeTypeDto =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "copied"
  | "type_changed"
  | "unmerged"
  | "untracked"
  | "ignored";

export type FileStatusCodeDto =
  | "unmodified"
  | "modified"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "updated_but_unmerged"
  | "untracked"
  | "ignored";

export interface FileChangeDto {
  path: string;
  previousPath: string | null;
  changeType: ChangeTypeDto;
  indexStatus: FileStatusCodeDto;
  worktreeStatus: FileStatusCodeDto;
}

export interface RepositoryStatusDto {
  branch: string | null;
  headState: HeadStateDto;
  files: FileChangeDto[];
  isClean: boolean;
}

// -- Commit graph (US-067) --------------------------------------------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s commit-graph section.
// `CommitGraphRowDto`/`GraphEdgeDto` are the Core-computed layout as-is —
// the frontend never recomputes a lane or edge itself (US-067 criterion 3).

export interface SignatureDto {
  name: string;
  email: string;
}

export interface GitTimestampDto {
  secondsSinceEpoch: number;
  utcOffsetMinutes: number;
}

export type DecorationDto =
  | { kind: "head" }
  | { kind: "branch"; name: string }
  | { kind: "remoteBranch"; remote: string; branch: string }
  | { kind: "tag"; name: string };

export interface CommitDto {
  hash: string;
  shortHash: string;
  parents: string[];
  author: SignatureDto;
  committer: SignatureDto;
  authorDate: GitTimestampDto;
  commitDate: GitTimestampDto;
  subject: string;
  body: string;
  decorations: DecorationDto[];
  isMerge: boolean;
  isRoot: boolean;
}

export interface GraphEdgeDto {
  fromLane: number;
  toLane: number;
  target: string;
  resolved: boolean;
}

// One rendered row: the commit it represents plus the lane/edge data the
// Core-computed layout carries for it (`gitsail_domain::graph`).
export interface CommitGraphRowDto {
  commit: CommitDto;
  lane: number;
  edges: GraphEdgeDto[];
  passthroughLanes: number[];
}

export interface CommitGraphPageDto {
  rows: CommitGraphRowDto[];
  laneCount: number;
  hasMore: boolean;
  nextCursor: string | null;
}

// -- Recent repositories (US-052) --------------------------------------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s `RecentRepositoryDto`,
// itself built from `gitsail_application::RecentRepositoryEntry`.

export interface RecentRepositoryDto {
  path: string;
  lastOpenedUnixSeconds: number;
}

// -- Patch export (US-029/T-162) ---------------------------------------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s `PatchExportDto`, itself
// built from `gitsail_application::PatchExport`. The frontend never
// renders or reconstructs the patch text itself — this is exactly what the
// `export_patch` command returned, ready to copy or save as-is.

export interface PatchExportDto {
  patch: string;
  includedFiles: string[];
  skippedBinaryFiles: string[];
  skippedTruncatedFiles: string[];
}

// -- Apply a patch (T-163/US-030) ---------------------------------------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s `PatchPreviewDto`/
// `ApplyPatchResultDto`, themselves built from
// `gitsail_application::write_ports::{PatchPreview, ApplyPatchResult}`.

export interface PatchPreviewDto {
  affectedFiles: string[];
  supported: boolean;
  rejectionReason: string | null;
}

export interface ApplyPatchResultDto {
  appliedFiles: string[];
}

// -- Branches (EPIC-12/T-189, T-193 local-branch subset) ----------------

export type BranchKindDto = { kind: "local" } | { kind: "remote"; remote: string };

export interface BranchDto {
  name: string;
  kind: BranchKindDto;
  target: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  isCurrent: boolean;
}

// -- Remotes and sync (EPIC-19/US-096..098; US-060/T-193's fetch/pull/push
// subset) -----------------------------------------------------------------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s remote/sync section.
// `RemoteDto`'s URLs are already redacted server-side (`RemoteUrl::
// redacted`) — the frontend never sees a raw credential to begin with.

export interface RemoteDto {
  name: string;
  fetchUrl: string;
  pushUrl: string;
}

/** What a sync action would target (or, for `fetch`/`push`, what it just
 * acted on) — `branch` is `null` only when no repository/branch context is
 * available at all; once resolved it is always the current branch, even
 * for `fetch` (which itself targets a whole remote, not one branch) since
 * that branch's upstream is what informed the remote choice. */
export interface SyncTargetDto {
  remote: string;
  branch: string | null;
}

export type PullOutcomeDto =
  | { outcome: "alreadyUpToDate" }
  | { outcome: "fastForwarded"; newHead: string };

export interface PullResultDto {
  remote: string;
  branch: string;
  outcome: PullOutcomeDto;
}

// -- Diffs (US-057/US-058): mirrors `gitsail_protocol::dto`'s diff
// section. `FileDiffDto` travels both ways — read from `get_diff`,
// trimmed down and echoed back to `stage_hunks`/`unstage_hunks` for a
// hunk-level selection (US-058 criterion 1).

export type DiffLineOriginDto = "context" | "addition" | "deletion";

export interface DiffLineDto {
  origin: DiffLineOriginDto;
  content: string;
  hasTrailingNewline: boolean;
}

export interface DiffHunkDto {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLineDto[];
}

export interface FileDiffDto {
  path: string;
  previousPath: string | null;
  changeType: ChangeTypeDto;
  isBinary: boolean;
  truncated: boolean;
  hunks: DiffHunkDto[];
}

export interface DiffDto {
  files: FileDiffDto[];
}

// -- Commit/amend results (US-058/US-059) --------------------------------

export interface CommitResultDto {
  hash: string;
}

export interface AmendPreviewDto {
  head: CommitDto;
  stagedDiff: DiffDto;
}

// -- Startup handoff (EPIC-15 gap closed by this epic; US-056) ----------
//
// Mirrors `gitsail_desktop_lib::state::StartupIntent` — the one DTO in
// this file whose Rust source of truth lives in the Desktop crate itself
// rather than `gitsail-protocol`, since it is a Desktop-process-only
// concept (argv), not a cross-surface protocol value.

export interface StartupIntentDto {
  repoPath: string | null;
  commitHash: string | null;
}
