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
