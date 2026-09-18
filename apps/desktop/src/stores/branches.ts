// Local-branch state (US-060's create/switch/delete subset; remote sync —
// fetch/pull/push — lives in `stores/sync.ts`). Every mutation goes through
// `stores/operation.ts` (T-194/US-061), so a create/switch/delete always
// shows its target and risk before running and can always be cancelled
// before it touches the repository.

import { defineStore } from "pinia";

import { createBranch, deleteBranch, listBranches, switchBranch } from "../services/branches";
import type { BranchDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useBranchesStore = defineStore("branches", {
  state: () => ({
    branches: [] as BranchDto[],
    isLoading: false,
    lastError: null as ErrorPayload | null,
  }),
  actions: {
    async load(): Promise<void> {
      this.isLoading = true;
      try {
        this.branches = await listBranches();
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
      }
    },

    /** Requests creating a local branch (Moderate risk, mirroring
     * `gitsail_tui::operation::OperationKind::CreateBranch`). `startPoint`
     * is any revision expression the Core can resolve; omit it to start
     * from `HEAD`. */
    async requestCreate(name: string, startPoint?: string): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "createBranch",
        risk: "moderate",
        targetLabel: `branch '${name}'`,
        run: async () => {
          await createBranch(name, startPoint);
          await this.load();
        },
      });
    },

    /** Requests switching to `target` (Moderate risk). A refresh of both
     * the branch list and the repository status follows a successful
     * switch, since HEAD and the working tree both just changed. */
    async requestSwitch(target: string): Promise<void> {
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "switchBranch",
        risk: "moderate",
        targetLabel: `branch '${target}'`,
        run: async () => {
          await switchBranch(target);
          await this.load();
          await session.refreshStatus("after_mutation");
        },
      });
    },

    /** Requests deleting `name`. Non-force is Moderate (Git itself refuses
     * an unmerged branch); `force: true` is Destructive and carries an
     * explicit, non-generic warning naming exactly what would be lost —
     * never a bare "are you sure?" (US-061 DoD). */
    async requestDelete(name: string, force: boolean): Promise<void> {
      const operation = useOperationStore();
      await operation.request({
        kind: "deleteBranch",
        risk: force ? "destructive" : "moderate",
        targetLabel: `branch '${name}'`,
        impact: force
          ? `This permanently deletes '${name}', including any commits only reachable from it and not merged elsewhere.`
          : undefined,
        run: async () => {
          await deleteBranch(name, force);
          await this.load();
        },
      });
    },
  },
});
