// Stage/unstage and commit composition (US-058). File lists are derived
// from `stores/session.ts`'s own `status` (the single source of truth for
// what is staged/unstaged, per SAD §21) rather than a second copy kept
// here — this store only adds the commit-message draft and the mutating
// actions, every one of which goes through `stores/operation.ts` (T-194).

import { defineStore } from "pinia";

import { createCommit, stageHunks as stageHunksCommand, stagePaths, unstageHunks as unstageHunksCommand, unstagePaths } from "../services/staging";
import type { FileChangeDto, FileDiffDto, FileStatusCodeDto } from "../services/dto";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

/** Mirrors `StatusPanel.vue`'s own `isRelevant` (US-046/US-029): the index
 * side has nothing to show for a purely untracked/ignored file, and the
 * working-tree side legitimately includes `untracked` but never
 * `unmodified`/`ignored`. Exported so both the composer and the status
 * panel apply the identical rule, rather than each re-deriving it. */
export function isRelevantChange(code: FileStatusCodeDto, scope: "staged" | "worktree"): boolean {
  if (scope === "staged") {
    return code !== "unmodified" && code !== "untracked" && code !== "ignored";
  }
  return code !== "unmodified" && code !== "ignored";
}

export const useStagingStore = defineStore("staging", {
  state: () => ({
    message: "",
    lastCommitHash: null as string | null,
  }),
  getters: {
    stagedFiles(): FileChangeDto[] {
      const session = useRepositorySessionStore();
      return (session.status?.files ?? []).filter((f) => isRelevantChange(f.indexStatus, "staged"));
    },
    unstagedFiles(): FileChangeDto[] {
      const session = useRepositorySessionStore();
      return (session.status?.files ?? []).filter((f) => isRelevantChange(f.worktreeStatus, "worktree"));
    },
  },
  actions: {
    async refreshAfterMutation(): Promise<void> {
      const session = useRepositorySessionStore();
      await session.refreshStatus("after_mutation");
    },

    /** Stages `paths` (Safe risk — mirrors
     * `gitsail_tui::operation::OperationKind::StageFiles` — so this runs
     * immediately without an extra confirmation click, matching every
     * other GitSail surface). */
    async stageFiles(paths: string[]): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "stageFiles",
        risk: "safe",
        targetLabel: paths.length === 1 ? `staged: ${paths[0]}` : `staged: ${paths.length} files`,
        run: async () => {
          await stagePaths(paths);
          await this.refreshAfterMutation();
        },
      });
    },

    async unstageFiles(paths: string[]): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "unstageFiles",
        risk: "safe",
        targetLabel: paths.length === 1 ? `unstaged: ${paths[0]}` : `unstaged: ${paths.length} files`,
        run: async () => {
          await unstagePaths(paths);
          await this.refreshAfterMutation();
        },
      });
    },

    async stageHunks(selection: FileDiffDto[]): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "stageHunks",
        risk: "safe",
        targetLabel: "selected hunks",
        run: async () => {
          await stageHunksCommand(selection);
          await this.refreshAfterMutation();
        },
      });
    },

    async unstageHunks(selection: FileDiffDto[]): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "unstageHunks",
        risk: "safe",
        targetLabel: "selected hunks",
        run: async () => {
          await unstageHunksCommand(selection);
          await this.refreshAfterMutation();
        },
      });
    },

    /** Requests committing exactly the current index content with
     * `this.message` (Moderate risk, mirrors
     * `OperationKind::CreateCommit`). On success, the message is cleared
     * and the new hash recorded; on failure/cancel, the typed message and
     * the current staged/unstaged selection are both left untouched
     * (US-058 criterion 3) — this action never clears `message` except on
     * the success path itself.
     */
    async requestCommit(): Promise<void> {
      const operation = useOperationStore();
      const stagedCount = this.stagedFiles.length;
      await operation.request({
        kind: "createCommit",
        risk: "moderate",
        targetLabel: "a new commit",
        impact: `Commits ${stagedCount} staged file${stagedCount === 1 ? "" : "s"} with the message below.`,
        run: async () => {
          const result = await createCommit(this.message);
          this.lastCommitHash = result.hash;
          this.message = "";
          await this.refreshAfterMutation();
        },
      });
    },
  },
});
