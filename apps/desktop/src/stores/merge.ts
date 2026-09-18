// Merge, conflict resolution, continue/abort, and rebase/skip state
// (EPIC-16/T-231..T-233; EPIC-17/T-235). Every mutation goes through
// `stores/operation.ts` (T-194/US-061), mirroring
// `stores/sync.ts`/`stores/branches.ts`'s own convention: a merge/rebase/
// continue/abort/skip always shows its target and risk before running, and
// can always be cancelled before it touches the repository.
//
// Rebase is folded into this same store rather than a separate one: it
// shares the exact same `inProgressOperation`/conflicts-resolution state
// (a rebase conflict is resolved through the same `markResolved`/`takeSide`
// flow a merge conflict already uses) and the same continue/abort actions
// below already dispatch generically regardless of which operation is
// actually pending — a second store would only duplicate that shared state,
// not add a distinct concern.
//
// Risk classification mirrors `gitsail_application::mutation::MutationKind`/
// `gitsail_tui::operation::OperationKind` exactly (SAD §20's own named
// `Moderate` example includes merge and, per this epic's own scope note,
// rebase): `merge`/`rebase`/`continueOperation`/`skipOperation` are
// Moderate, `abortOperation` is Destructive (it discards the in-progress
// operation's own changes), and marking a conflict resolved is Safe (it is
// `git add`, mirroring `stageFiles`) — so it dispatches without a
// confirmation step, exactly like `stores/branches.ts`'s own Safe actions.
//
// `inProgressOperation` is never inferred from this store's own last
// action — it is always freshly re-read via `refreshInProgressOperation`
// after every mutation here, so an operation started outside GitSail, or
// the real aftermath of a continue/abort/skip this session just ran, is
// always what is actually shown (US-078 criterion 2; US-081 criterion 3;
// US-083 criterion 3: "never presume success without checking").

import { defineStore } from "pinia";

import {
  abortOperation as abortOperationCommand,
  continueOperation as continueOperationCommand,
  detectInProgressOperation,
  getConflictSides,
  markConflictResolved as markConflictResolvedCommand,
  merge as mergeCommand,
  rebase as rebaseCommand,
  skipOperation as skipOperationCommand,
  takeConflictSide as takeConflictSideCommand,
} from "../services/merge";
import type {
  ConflictedFileDto,
  ConflictSidesDto,
  InProgressOperationDto,
  MergeResultDto,
  OperationCapabilityDto,
  RebaseResultDto,
} from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

const NONE: InProgressOperationDto = { kind: "none" };

/** The conflicted files a given [`InProgressOperationDto`] carries, or an
 * empty list for `"none"` — mirrors `gitsail_domain::InProgressOperation::
 * conflicted_files`. */
export function conflictedFilesOf(operation: InProgressOperationDto): ConflictedFileDto[] {
  return operation.kind === "none" ? [] : operation.conflictedFiles;
}

/** The capabilities a given [`InProgressOperationDto`] offers, or none for
 * `"none"` — mirrors `gitsail_domain::InProgressOperation::capabilities`. */
export function capabilitiesOf(operation: InProgressOperationDto): OperationCapabilityDto[] {
  return operation.kind === "none" ? [] : operation.capabilities;
}

export const useMergeStore = defineStore("merge", {
  state: () => ({
    inProgressOperation: NONE as InProgressOperationDto,
    isLoadingOperation: false,
    lastMergeResult: null as MergeResultDto | null,
    /** The outcome of the last successful `requestRebase` (T-235/US-083
     * criterion 3: completion and conflict are always two distinct,
     * explicit outcomes) — mirrors `lastMergeResult`'s own convention. */
    lastRebaseResult: null as RebaseResultDto | null,
    /** The conflicted file most recently inspected (T-232/US-080 criterion
     * 2), or `null` before anything has been inspected, or once the
     * highlighted file/operation changes. */
    inspectedPath: null as string | null,
    inspectedSides: null as ConflictSidesDto | null,
    /** A failure inspecting or resolving a conflict — side-channel like
     * `stores/sync.ts`'s `resolveError`, not modeled through
     * `useOperationStore` since mark-resolved/take-side dispatch
     * immediately (Safe-risk convention) rather than through a confirm
     * step. */
    conflictError: null as ErrorPayload | null,
  }),
  getters: {
    conflictedFiles: (state): ConflictedFileDto[] => conflictedFilesOf(state.inProgressOperation),
    capabilities: (state): OperationCapabilityDto[] => capabilitiesOf(state.inProgressOperation),
    hasConflicts(): boolean {
      return this.conflictedFiles.length > 0;
    },
    supportsContinue(): boolean {
      return this.capabilities.includes("continue");
    },
    supportsAbort(): boolean {
      return this.capabilities.includes("abort");
    },
    /** T-235/US-083 criterion 3: a merge never offers this (it has no
     * further step to skip past) — only a rebase/cherry-pick/revert/bisect
     * sequencer step does. */
    supportsSkip(): boolean {
      return this.capabilities.includes("skip");
    },
  },
  actions: {
    /** Re-reads whatever merge/rebase/cherry-pick/revert/bisect is
     * currently pending (T-230/US-078), never from cached/assumed state.
     * Called after every mutation in this store, and should also be called
     * once on repository open/focus alongside the rest of the session
     * refresh. */
    async refreshInProgressOperation(): Promise<void> {
      this.isLoadingOperation = true;
      try {
        const operation = await detectInProgressOperation();
        if (operation.kind === "none") {
          this.inspectedPath = null;
          this.inspectedSides = null;
          this.conflictError = null;
        }
        this.inProgressOperation = operation;
      } catch {
        // Detecting in-progress-operation state is a display convenience
        // alongside the rest of the session refresh; a failure here does
        // not block the repository from otherwise being usable.
        this.inProgressOperation = NONE;
      } finally {
        this.isLoadingOperation = false;
      }
    },

    /** Requests merging `targetRevision` into the current branch (T-231/
     * US-079 criterion 1: origin — the current branch — destination and
     * policy — a plain, non-force merge — are all named by `targetLabel`
     * before anything runs). Fast-forward, a new merge commit, and a
     * conflict are always three distinct, explicit `MergeResultDto`
     * outcomes (criterion 2) — never a generic success/failure. */
    async requestMerge(targetRevision: string): Promise<void> {
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "merge",
        risk: "moderate",
        targetLabel: `merging '${targetRevision}' into the current branch`,
        run: async () => {
          this.lastMergeResult = await mergeCommand(targetRevision);
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
    },

    /** Requests rebasing the current branch onto `ontoRevision` (T-235/
     * US-083 criterion 1: the current branch, the chosen base, and the
     * fact that this reapplies the branch's own commits are all named by
     * `targetLabel` before anything runs). Never silently stashes local
     * changes — a dirty working tree comes back as an ordinary failure
     * from `rebaseCommand` (US-083 criterion 2), reported the same way any
     * other refused operation is. Completion and conflict are always two
     * distinct, explicit `RebaseResultDto` outcomes (criterion 3), mirroring
     * `requestMerge`'s own reasoning. */
    async requestRebase(ontoRevision: string): Promise<void> {
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "rebase",
        risk: "moderate",
        targetLabel: `rebasing the current branch onto '${ontoRevision}'`,
        run: async () => {
          this.lastRebaseResult = await rebaseCommand(ontoRevision);
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
    },

    /** Loads `path`'s base/ours/theirs sides for inspection (T-232/US-080
     * criterion 2). */
    async inspectConflict(path: string): Promise<void> {
      try {
        this.inspectedSides = await getConflictSides(path);
        this.inspectedPath = path;
        this.conflictError = null;
      } catch (error) {
        this.conflictError = toErrorPayload(error);
      }
    },

    /** Marks `path` resolved by staging its current working-tree content
     * (T-232/US-080 criterion 3) — only ever this explicit call, never
     * inferred from the file merely "looking" resolved. `Safe` risk (it is
     * `git add`), so this dispatches without a confirmation step, mirroring
     * `stores/branches.ts`'s own Safe actions. */
    async markResolved(path: string): Promise<void> {
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "markConflictResolved",
        risk: "safe",
        targetLabel: `'${path}' as resolved`,
        run: async () => {
          await markConflictResolvedCommand(path);
          if (this.inspectedPath === path) {
            this.inspectedSides = null;
            this.inspectedPath = null;
          }
          this.conflictError = null;
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
      // A failure surfaces generically through `useOperationStore` (the
      // same overlay every other mutation's failure shows) — this store's
      // own `conflictError` is reserved for a read-only `inspectConflict`
      // failure, which has no operation of its own to report through.
    },

    /** Resolves `path` by taking `side` wholesale — the documented
     * binary-conflict flow (T-232/US-080 criterion 3), equally usable for a
     * text file. `Moderate` risk: it overwrites the working-tree content,
     * discarding whatever it held before, but the discarded side remains
     * recoverable via `inspectConflict` until the operation concludes. */
    async takeSide(path: string, side: "ours" | "theirs"): Promise<void> {
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "takeConflictSide",
        risk: "moderate",
        targetLabel: `'${path}' (take ${side})`,
        run: async () => {
          await takeConflictSideCommand(path, side);
          if (this.inspectedPath === path) {
            this.inspectedSides = null;
            this.inspectedPath = null;
          }
          this.conflictError = null;
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
    },

    /** Requests continuing the pending operation (T-233/US-081 criterion 1:
     * only offered when `supportsContinue` is true — a no-op otherwise). */
    async requestContinue(): Promise<void> {
      if (!this.supportsContinue) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "continueOperation",
        risk: "moderate",
        targetLabel: "the in-progress operation",
        run: async () => {
          await continueOperationCommand();
          await session.refreshStatus("after_mutation");
          // T-233 criterion 3: the real resulting state is reinspected
          // here, never presumed from `continueOperationCommand` resolving
          // without throwing.
          await this.refreshInProgressOperation();
        },
      });
    },

    /** Requests aborting the pending operation (T-233/US-081 criterion 1).
     * `Destructive` risk: it discards the in-progress operation's own
     * changes (e.g. a merge's conflict resolutions in progress), even
     * though Git restores the pre-operation state rather than losing
     * history outright. */
    async requestAbort(): Promise<void> {
      if (!this.supportsAbort) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "abortOperation",
        risk: "destructive",
        targetLabel: "the in-progress operation",
        impact:
          "This restores HEAD to before the operation started and discards its own in-progress changes — unrelated local work is left untouched.",
        run: async () => {
          await abortOperationCommand();
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
    },

    /** Requests skipping the current step of the pending operation (T-235/
     * US-083 criterion 3) — only offered when `supportsSkip` is true (a
     * no-op otherwise, mirroring `requestContinue`/`requestAbort`'s own
     * capability-gated convention). `Moderate` risk: deliberately advancing
     * past an already-confirmed, in-progress operation's current step,
     * the same character `requestContinue` already has. */
    async requestSkip(): Promise<void> {
      if (!this.supportsSkip) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "skipOperation",
        risk: "moderate",
        targetLabel: "the current step of the in-progress operation",
        run: async () => {
          await skipOperationCommand();
          await session.refreshStatus("after_mutation");
          await this.refreshInProgressOperation();
        },
      });
    },
  },
});
