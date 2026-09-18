// The typed frontend service for merge, conflict resolution, and continue/
// abort (EPIC-16/T-231..T-233): every call here is a thin pass-through to a
// Tauri command that itself only calls `gitsail-application` (AGENTS.md),
// mirroring `services/sync.ts`'s own convention. No Git logic lives here.

import { invoke } from "@tauri-apps/api/core";

import type {
  ConflictSidesDto,
  InProgressOperationDto,
  MergeResultDto,
} from "./dto";

/** Detects a merge/rebase/cherry-pick/revert/bisect currently in progress
 * (T-230/US-078), read-only. Never inferred from the Desktop's own last
 * action — always freshly re-read, so an operation started in another
 * terminal, or the real aftermath of a continue/abort this session just
 * ran, is always what is reported (US-081 criterion 3). */
export async function detectInProgressOperation(): Promise<InProgressOperationDto> {
  return invoke<InProgressOperationDto>("detect_in_progress_operation");
}

/** Integrates `targetRevision` (a branch, tag, or other Git revision
 * expression) into the current branch (T-231/US-079). Fast-forward, a new
 * merge commit, and a conflict are always three distinct, explicit
 * outcomes — never collapsed into one another, and a conflict is never
 * thrown as an error. */
export async function merge(targetRevision: string): Promise<MergeResultDto> {
  return invoke<MergeResultDto>("merge", { targetRevision });
}

/** Reads one conflicted file's base/ours/theirs sides (T-232/US-080
 * criterion 2), read-only. */
export async function getConflictSides(path: string): Promise<ConflictSidesDto> {
  return invoke<ConflictSidesDto>("get_conflict_sides", { path });
}

/** Marks a conflicted file resolved by staging its current working-tree
 * content (T-232/US-080 criterion 3) — only ever this explicit call, never
 * inferred from the file merely "looking" resolved. */
export async function markConflictResolved(path: string): Promise<void> {
  await invoke<void>("mark_conflict_resolved", { path });
}

/** Resolves a conflicted file by taking `side` wholesale — the documented
 * binary-conflict flow (T-232/US-080 criterion 3), equally usable for a
 * text file a person simply wants to resolve by taking one side outright. */
export async function takeConflictSide(path: string, side: "ours" | "theirs"): Promise<void> {
  await invoke<void>("take_conflict_side", { path, side });
}

/** Resumes whichever operation is currently pending (T-233/US-081). The
 * real resulting state is never presumed from this call's own success —
 * the caller re-calls `detectInProgressOperation` afterward to see it
 * (criterion 3). */
export async function continueOperation(): Promise<void> {
  await invoke<void>("continue_operation");
}

/** Abandons whichever operation is currently pending (T-233/US-081),
 * restoring the pre-operation state as far as Git itself guarantees.
 * Matches `continueOperation`'s own "never presumed, always reinspected"
 * contract. */
export async function abortOperation(): Promise<void> {
  await invoke<void>("abort_operation");
}
