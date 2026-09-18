// The typed frontend service for merge, conflict resolution, and continue/
// abort (EPIC-16/T-231..T-233): every call here is a thin pass-through to a
// Tauri command that itself only calls `gitsail-application` (AGENTS.md),
// mirroring `services/sync.ts`'s own convention. No Git logic lives here.

import { invoke } from "@tauri-apps/api/core";

import type {
  CherryPickResultDto,
  ConflictSidesDto,
  InProgressOperationDto,
  MergeParentPolicyDto,
  MergeResultDto,
  RebasePlanDto,
  RebaseResultDto,
  ResetModeDto,
  RevertResultDto,
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

/** Rebases the current branch onto `ontoRevision` (T-235/US-083). Never
 * silently stashes uncommitted changes — a dirty working tree or any other
 * incompatible state comes back as a rejected promise instead. Completion
 * and conflict are always two distinct, explicit outcomes (criterion 3),
 * mirroring `merge`'s own contract. */
export async function rebase(ontoRevision: string): Promise<RebaseResultDto> {
  return invoke<RebaseResultDto>("rebase", { ontoRevision });
}

/** Skips the current step of whichever operation is pending (T-235/US-083
 * criterion 3) and moves to the next one. Rejects with a clear error when
 * the detected operation does not offer skip (e.g. a merge, which has no
 * further step to skip past). Matches `continueOperation`'s own "never
 * presumed, always reinspected" contract. */
export async function skipOperation(): Promise<void> {
  await invoke<void>("skip_operation");
}

/** Reads a non-mutating interactive rebase plan for the candidate range the
 * current branch would reapply onto `ontoRevision` (T-236/US-084 criterion
 * 1). Read-only: never touches the working tree, the index, or any ref. */
export async function planRebase(ontoRevision: string): Promise<RebasePlanDto> {
  return invoke<RebasePlanDto>("plan_rebase", { ontoRevision });
}

/** Applies a previously built/edited interactive rebase plan (T-236/US-084;
 * T-237/US-085's squash/fixup are just two of this same plan's actions).
 * `onto`/`branchHead` are revalidated by the Core immediately before
 * applying anything (criterion 2) — a stale plan rejects with a clear error
 * rather than silently rebuilding itself. */
export async function executeRebasePlan(plan: RebasePlanDto): Promise<RebaseResultDto> {
  return invoke<RebaseResultDto>("execute_rebase_plan", { plan });
}

/** Applies `commit`'s change onto the current branch (T-238/US-086).
 * `mergeParent` must be `"firstParent"` when `commit` is a merge commit
 * (US-086 criterion 2) — omitted against a merge commit is refused by the
 * Core itself, never guessed here. Applying, a conflict, and an empty
 * "already applied" result are always three distinct, explicit outcomes
 * (criterion 3) — never thrown as an opaque error for the latter two. */
export async function cherryPick(
  commit: string,
  mergeParent?: MergeParentPolicyDto,
): Promise<CherryPickResultDto> {
  return invoke<CherryPickResultDto>("cherry_pick", { commit, mergeParent: mergeParent ?? null });
}

/** Creates a new commit undoing `commit`'s change (T-239/US-087) — never
 * rewrites or moves any existing reference. `mergeParent` mirrors
 * `cherryPick`'s own contract. Completion and conflict are always two
 * distinct, explicit outcomes (criterion 2). */
export async function revert(
  commit: string,
  mergeParent?: MergeParentPolicyDto,
): Promise<RevertResultDto> {
  return invoke<RevertResultDto>("revert", { commit, mergeParent: mergeParent ?? null });
}

/** Moves `HEAD` (and, per `mode`, the index/working tree) to
 * `targetRevision` (T-240/US-088). `expectedHead` must be the exact hash
 * last observed as `HEAD` when the reset was previewed/confirmed — the Core
 * revalidates it is still `HEAD` immediately before resetting and rejects
 * with a classified conflict otherwise (criterion 3). */
export async function reset(
  targetRevision: string,
  mode: ResetModeDto,
  expectedHead: string,
): Promise<void> {
  await invoke<void>("reset", { targetRevision, mode, expectedHead });
}
