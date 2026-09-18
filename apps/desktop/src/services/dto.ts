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

// -- EPIC-16/T-231..T-233: merge, in-progress-operation, conflicts ---------
//
// Mirrors `crates/gitsail-protocol/src/dto.rs`'s own merge/conflicts
// section one-to-one, so Desktop and `gitsail-tui` never drift on what a
// conflict/capability/merge outcome means.

export type ConflictStageDto =
  | "bothModified"
  | "bothAdded"
  | "bothDeleted"
  | "addedByUs"
  | "addedByThem"
  | "deletedByUs"
  | "deletedByThem";

export interface ConflictedFileDto {
  path: string;
  stage: ConflictStageDto;
}

export type OperationCapabilityDto = "continue" | "skip" | "abort";

/** Which multi-step Git operation, if any, is currently in progress
 * (T-230/US-078) — `"none"` is its own explicit variant rather than `null`,
 * so "nothing pending" is exactly as explicit on the wire as every other
 * state. */
export type InProgressOperationDto =
  | { kind: "none" }
  | {
      kind: "merge";
      heads: string[];
      conflictedFiles: ConflictedFileDto[];
      capabilities: OperationCapabilityDto[];
    }
  | {
      kind: "rebase";
      interactive: boolean;
      onto: string | null;
      conflictedFiles: ConflictedFileDto[];
      capabilities: OperationCapabilityDto[];
    }
  | {
      kind: "cherryPick";
      target: string | null;
      conflictedFiles: ConflictedFileDto[];
      capabilities: OperationCapabilityDto[];
    }
  | {
      kind: "revert";
      target: string | null;
      conflictedFiles: ConflictedFileDto[];
      capabilities: OperationCapabilityDto[];
    }
  | {
      kind: "bisectRun";
      conflictedFiles: ConflictedFileDto[];
      capabilities: OperationCapabilityDto[];
    };

/** A merge's exact outcome (T-231/US-079 criterion 2): fast-forward, a new
 * merge commit, and a conflict are always three distinct, explicit
 * variants — a conflict is never collapsed into a bare success. */
export type MergeResultDto =
  | { outcome: "fastForwarded"; newHead: string }
  | { outcome: "mergeCommitCreated"; hash: string }
  | { outcome: "conflict"; conflictedFiles: ConflictedFileDto[] };

/** A rebase's exact outcome (T-235/US-083 criterion 3): completion and a
 * conflict are always two distinct, explicit variants — a conflict is never
 * collapsed into a bare success, mirroring `MergeResultDto`'s own
 * convention. */
export type RebaseResultDto =
  | { outcome: "completed"; newHead: string }
  | { outcome: "conflict"; conflictedFiles: ConflictedFileDto[] };

/** One action assignable to a rebase plan entry (T-236/US-084 criterion 1).
 * `"edit"` is deliberately not modeled — see `RebaseAction`'s own doc in
 * `gitsail-application`: every other action a person would reach for before
 * sharing history is covered, and stopping mid-rebase to hand-edit a
 * commit's content is a materially larger, separate capability. */
export type RebaseActionDto = "pick" | "reword" | "squash" | "fixup" | "drop";

/** One commit's position and assigned action within a rebase plan (T-236/
 * US-084). `messageOverride` is only ever meaningful for `"reword"` —
 * `execute_rebase_plan`'s own revalidation is the final authority on that
 * rule, never this shape alone. */
export interface RebasePlanEntryDto {
  commit: string;
  shortHash: string;
  subject: string;
  action: RebaseActionDto;
  messageOverride: string | null;
}

/** A non-mutating interactive rebase plan (T-236/US-084 criterion 1): the
 * candidate commit range the current branch would reapply onto
 * `ontoRevision`, oldest first, each defaulted to `"pick"` until reassigned.
 * `onto`/`branchHead` are the exact state the plan was built against —
 * `execute_rebase_plan` revalidates both immediately before applying
 * anything (criterion 2), refusing a stale plan rather than silently
 * rebuilding it. */
export interface RebasePlanDto {
  ontoRevision: string;
  onto: string;
  branchHead: string;
  entries: RebasePlanEntryDto[];
}

/** One conflict side's content (T-232/US-080 criterion 2) — `"absent"` is a
 * legitimate, expected outcome (e.g. no common ancestor for a file added
 * independently on both sides), never an error. */
export type ConflictSideContentDto =
  | { kind: "text"; text: string }
  | { kind: "binary" }
  | { kind: "absent" };

export interface ConflictSidesDto {
  path: string;
  base: ConflictSideContentDto;
  ours: ConflictSideContentDto;
  theirs: ConflictSideContentDto;
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
