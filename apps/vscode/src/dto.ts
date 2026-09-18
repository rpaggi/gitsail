// Hand-written mirror of the wire shapes `gitsail-protocol` defines in
// `crates/gitsail-protocol/src/dto.rs` (source of truth). Shape mirrors
// only, never logic: no Git rule is reimplemented here (US-069 criterion 2),
// just field names/types matching the Rust DTOs' `#[serde(rename_all =
// "camelCase")]` wire format.
//
// This is now the *third* hand-written copy of these shapes (the Rust
// source of truth, `apps/desktop/src/services/dto.ts`, and this file) —
// `apps/desktop`'s own dto.ts already flags this exact risk ("if a second
// consumer... makes hand-maintaining two clients painful, revisit codegen").
// A third copy makes that revisit overdue: if EPIC-15's blame/history DTOs
// push this file much further, adopt a shared codegen tool (e.g. ts-rs)
// instead of adding a fourth hand-written mirror.
//
// Only what EPIC-14 (repository identity) needs is mirrored so far.

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
