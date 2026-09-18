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
