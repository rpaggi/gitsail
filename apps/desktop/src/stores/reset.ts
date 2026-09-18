// Reset mode chooser and confirmation (T-240/US-088). Every mutation goes
// through `stores/operation.ts` (T-194/US-061), mirroring `stores/merge.ts`/
// `stores/amend.ts`'s own convention: soft/mixed/hard each show their own
// distinct effect on HEAD/the index/the working tree before anything runs
// (criterion 1), and `hard` carries the concrete, live count of uncommitted
// changes it would permanently discard as its `impact` text (criterion 2) —
// never a generic "are you sure?" warning.
//
// `expectedHead` is read from the already-loaded `stores/branches.ts` list
// (the current branch's own `target`) rather than a second, dedicated read
// — the same commit hash `stores/branches.ts` already keeps fresh via its
// own `load()` — and is revalidated by the Core (`RepositoryWritePort::reset`)
// immediately before the reset actually runs (US-088 criterion 3): a HEAD
// that moves between opening the chooser and confirming is caught there,
// never executed against silently.

import { defineStore } from "pinia";

import { reset as resetCommand } from "../services/merge";
import type { ResetModeDto } from "../services/dto";
import { useBranchesStore } from "./branches";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

/** The commit a reset targets, carrying just enough to build a concrete
 * (never generic) confirmation label. */
export interface ResetTarget {
  hash: string;
  shortHash: string;
}

function resetModeDescription(mode: ResetModeDto): string {
  switch (mode) {
    case "soft":
      return "soft — HEAD moves only; index and working tree preserved (changes become staged)";
    case "mixed":
      return "mixed — HEAD and index move; working tree preserved (changes become unstaged)";
    case "hard":
      return "HARD — HEAD, index and working tree all move";
  }
}

export const useResetStore = defineStore("reset", {
  state: () => ({
    /** The commit the chooser is currently open against, or `null` while
     * closed (US-088 criterion 1: opening the chooser is not itself a
     * mutation). */
    target: null as ResetTarget | null,
    mode: "soft" as ResetModeDto,
  }),
  getters: {
    isOpen: (state): boolean => state.target !== null,
    /** The concrete count of uncommitted changes a `hard` reset would
     * permanently discard right now (US-088 criterion 2) — read from the
     * already-loaded repository status, never a second read and never a
     * generic "some changes" estimate. Live: it reflects whatever
     * `stores/session.ts` currently holds, so it stays accurate even if the
     * chooser has been open a while and the working tree changed under it. */
    predictedLossFileCount(): number {
      return useRepositorySessionStore().status?.files.length ?? 0;
    },
  },
  actions: {
    /** Opens the chooser for `target`, defaulting to `soft` — mirrors
     * `gitsail-tui`'s own reset-mode chooser default. */
    open(target: ResetTarget): void {
      this.target = target;
      this.mode = "soft";
    },

    /** Closes the chooser without running anything (US-088's own
     * "cancelling never touches the repository", matching every other
     * mutation in this workspace). */
    close(): void {
      this.target = null;
    },

    setMode(mode: ResetModeDto): void {
      this.mode = mode;
    },

    /** Confirms the chooser's current target/mode, starting the reinforced
     * confirmation (US-088 criteria 1, 2). A no-op without an open chooser,
     * or when the current branch's tip (`expectedHead`) is not yet known —
     * there is nothing safe to revalidate against. The chooser is closed
     * the moment this dispatches, mirroring `stores/merge.ts`'s own rebase
     * plan overlay convention: a failed/stale confirmation is never
     * followed by silently reopening the same chooser. */
    async requestReset(): Promise<void> {
      const target = this.target;
      if (!target) {
        return;
      }
      const currentBranch = useBranchesStore().branches.find(
        (branch) => branch.isCurrent && branch.kind.kind === "local",
      );
      if (!currentBranch) {
        return;
      }
      const mode = this.mode;
      const predictedLoss = this.predictedLossFileCount;
      const expectedHead = currentBranch.target;
      this.target = null;

      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "reset",
        risk: mode === "hard" ? "destructive" : "moderate",
        targetLabel: `resetting to '${target.shortHash}' (${resetModeDescription(mode)})`,
        impact:
          mode === "hard"
            ? `${predictedLoss} uncommitted change${predictedLoss === 1 ? "" : "s"} will be permanently discarded.`
            : undefined,
        run: async () => {
          await resetCommand(target.hash, mode, expectedHead);
          await session.refreshStatus("after_mutation");
        },
      });
    },
  },
});
