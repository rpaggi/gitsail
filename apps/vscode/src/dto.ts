// The shapes every presentation module in this extension consumes
// (`blameFormat.ts`, `historyPresentation.ts`, `commitDetailsText.ts`,
// `historyController.ts`), still field-for-field identical to
// `crates/gitsail-protocol/src/dto.rs`.
//
// **What these are now, after ADR-025.** They used to be a mirror of a wire
// format: this extension received them as JSON from `gitsail-cli --json`
// and only had to agree with the Rust DTOs about field names and types. It
// no longer receives them from anywhere — `src/git/` *produces* them from
// `git`'s own output. So agreement with the Rust side is no longer a
// deserialization concern that a mismatch would announce loudly; it is a
// semantic one, and a divergence would be silent: the same commit rendering
// with a different author, a different short hash, or a different diff base
// in VS Code than in the TUI.
//
// That is precisely the cost ADR-025 accepts, and the reason these shapes
// were kept rather than redesigned around what `git` happens to emit. Two
// things hold the line: these types stay a faithful copy of `dto.rs`, and
// `test/gitParity.test.ts` runs the real `gitsail` CLI against the same
// temporary repository as `src/git/` and asserts the two produce the same
// DTOs. Changing a field here without changing `dto.rs` — or the reverse —
// is a bug, not a local decision.
//
// One field is no longer filled the same way: `PageDto.nextCursor` remains
// an opaque cursor, but nothing outside `src/git/gitClient.ts` may assume
// what is inside it (it is an offset today, exactly as `gitsail log`'s was).
//
// EPIC-14 (repository identity) plus EPIC-15 (blame/history/diff for the
// editor) are covered so far.

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

// ---------------------------------------------------------------------
// Commits (EPIC-15). Mirrors `gitsail_protocol::{SignatureDto,
// GitTimestampDto, DecorationDto, CommitDto}` — only the fields this
// extension actually renders are given real attention below; the rest are
// mirrored structurally so `CommitDto` type-checks end to end.
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// Diffs (EPIC-15). Mirrors `gitsail_protocol::{ChangeTypeDto,
// DiffLineOriginDto, DiffLineDto, DiffHunkDto, FileDiffDto, DiffDto}`.
//
// Note the *different* serde casing conventions this mirrors exactly:
// `ChangeTypeDto`/`DiffLineOriginDto` are Rust `#[serde(rename_all =
// "snake_case")]` enums (wire values like `"type_changed"`), while every
// struct *field* name in this file is `camelCase` — these are two
// independent casing choices in the Rust source, not a typo here.
// ---------------------------------------------------------------------

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

/** The diff of a single commit against its resolved base (US-026/US-076):
 * `base` is `null` for a root commit (diffed against the empty tree) and the
 * first-parent hash otherwise — this extension never re-derives that policy
 * itself, it only ever displays what `gitsail commit-diff` already decided
 * (T-209 criterion 1). */
export interface CommitDiffDto {
  target: string;
  base: string | null;
  diff: DiffDto;
}

// ---------------------------------------------------------------------
// Blame (EPIC-07/EPIC-15). Mirrors `gitsail_protocol::{BlameOriginDto,
// BlameLineDto, BlameDto}`.
// ---------------------------------------------------------------------

export type BlameOriginDto = "committed" | "local";

export interface BlameLineDto {
  finalLine: number;
  originalLine: number;
  commit: string;
  author: SignatureDto;
  timestamp: GitTimestampDto;
  content: string;
  origin: BlameOriginDto;
}

export interface BlameDto {
  file: string;
  revision: string | null;
  lines: BlameLineDto[];
}

// ---------------------------------------------------------------------
// Line history (EPIC-04/EPIC-15). Mirrors `gitsail_protocol::{LineRangeDto,
// LineHistoryEntryDto, LineHistoryDto}`.
// ---------------------------------------------------------------------

export interface LineRangeDto {
  start: number;
  end: number;
}

export interface LineHistoryEntryDto {
  commit: CommitDto;
  hunks: DiffHunkDto[];
}

export interface LineHistoryDto {
  file: string;
  revision: string;
  range: LineRangeDto;
  entries: LineHistoryEntryDto[];
}

// ---------------------------------------------------------------------
// File content at a revision (EPIC-15/US-076). Mirrors
// `gitsail_protocol::FileContentDto` — a tagged union, like `HeadStateDto`:
// `binary`/`missing` are legitimate, expected outcomes (never an error), so
// this extension must handle all three `kind`s explicitly rather than
// assuming `content` is always present.
// ---------------------------------------------------------------------

export type FileContentDto =
  | { kind: "text"; path: string; revision: string; content: string }
  | { kind: "binary"; path: string; revision: string }
  | { kind: "missing"; path: string; revision: string };

// ---------------------------------------------------------------------
// Pagination envelope. Mirrors `gitsail_protocol::Page<T>` (used by
// `gitsail log`, EPIC-15's file-history browsing).
// ---------------------------------------------------------------------

export interface PageDto<T> {
  items: T[];
  nextCursor?: string;
  hasMore: boolean;
}
