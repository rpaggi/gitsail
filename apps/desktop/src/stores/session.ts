// Repository session state (SAD §17, §21). Frontend mirror of the backend's
// `gitsail_application::RepositorySession`, extended for US-052/US-054 with
// a `generation` counter (this store's equivalent of the backend
// `AppState` session epoch) plus the async orchestration for opening a
// repository and refreshing its status — following the same
// state-plus-actions-in-one-store shape `stores/graph.ts` (EPIC-13)
// established, rather than splitting transient flags into `view.ts`.
//
// `generation` is bumped by every `openRepository` call, *before* the
// `open_repository` invoke even starts (US-052 criterion 3; US-054
// criterion 3). Any response still in flight for a previous repository —
// a status refresh, a commit-graph page, anything reading through this
// session — captured an older generation number and must be discarded by
// its own caller (via `isCurrent`) rather than overwriting what the user
// is now looking at. This is the one mechanism both stories' "isolate a
// switched-away session" criterion shares; the backend has its own,
// independent epoch guard for state it owns (`AppState`'s commit graph),
// but nothing here depends on the two ever being numerically equal — each
// side only ever compares its own counter against itself.

import { defineStore } from "pinia";

import {
  getRepositoryStatus,
  openRepository as openRepositoryCommand,
  type RefreshReason,
} from "../services/repository";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import type { RepositoryDto, RepositoryStatusDto } from "../services/dto";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export type OpenRepositoryResult =
  | { status: "opened" }
  | { status: "superseded" }
  | { status: "failed"; error: ErrorPayload };

export const useRepositorySessionStore = defineStore("repositorySession", {
  state: () => ({
    repository: null as RepositoryDto | null,
    status: null as RepositoryStatusDto | null,
    generation: 0,
    isOpening: false,
    isRefreshing: false,
    lastError: null as ErrorPayload | null,
  }),
  getters: {
    // Whether `generation` is still this session's current one — the
    // guard every async continuation below re-checks before touching
    // `repository`/`status`/`lastError`.
    isCurrent: (state) => (generation: number): boolean => generation === state.generation,
  },
  actions: {
    /**
     * Opens `path` as the active repository, replacing whatever was open
     * before (US-052 criterion 1). Bumps `generation` synchronously,
     * before the `open_repository` invoke even starts, so a second
     * `openRepository` call issued while this one is still in flight
     * always wins — this call's own eventual result is then reported as
     * `"superseded"` and never touches `repository`/`status`/`lastError`
     * (US-052 criterion 3; US-054 criterion 3).
     */
    async openRepository(path: string): Promise<OpenRepositoryResult> {
      const generation = ++this.generation;
      this.repository = null;
      this.status = null;
      this.lastError = null;
      this.isOpening = true;
      // `isRefreshing` reflects the *current* generation only. Without
      // resetting it here, a still-in-flight refresh from the superseded
      // generation would make the chained `refreshStatus` call below
      // think a refresh is already running and skip itself entirely —
      // the stale refresh's own `finally` is a no-op once superseded (see
      // `isCurrent` there), so nothing else clears this flag for us.
      this.isRefreshing = false;
      try {
        const repository = await openRepositoryCommand(path);
        if (!this.isCurrent(generation)) {
          return { status: "superseded" };
        }
        this.repository = repository;
        await this.refreshStatus("manual", generation);
        return { status: "opened" };
      } catch (error) {
        const payload = toErrorPayload(error);
        if (!this.isCurrent(generation)) {
          return { status: "superseded" };
        }
        this.lastError = payload;
        return { status: "failed", error: payload };
      } finally {
        if (this.isCurrent(generation)) {
          this.isOpening = false;
        }
      }
    },

    /**
     * Refreshes the status of the currently open repository (US-054
     * criterion 2): manual refresh, window-focus refresh, and a future
     * post-mutation refresh (once a mutation command exists) all call
     * this one action, so there is exactly one status-reading code path —
     * none of them re-implements its own fetch.
     *
     * `expectedGeneration` defaults to the session's current generation;
     * `openRepository` passes the generation it captured so a switch that
     * happens mid-open is still honored (the chained refresh below is for
     * that same, still-current, generation).
     *
     * A no-op when: no repository is open yet, `expectedGeneration` is
     * already stale, or a refresh is already in flight (US-054 criterion
     * 1: refreshing never queues up duplicate reads).
     */
    async refreshStatus(
      reason: RefreshReason = "manual",
      expectedGeneration?: number,
    ): Promise<void> {
      const generation = expectedGeneration ?? this.generation;
      if (this.repository === null || !this.isCurrent(generation) || this.isRefreshing) {
        return;
      }
      this.isRefreshing = true;
      try {
        const status = await getRepositoryStatus(reason);
        if (this.isCurrent(generation)) {
          this.status = status;
          this.lastError = null;
        }
      } catch (error) {
        if (this.isCurrent(generation)) {
          this.lastError = toErrorPayload(error);
        }
      } finally {
        if (this.isCurrent(generation)) {
          this.isRefreshing = false;
        }
      }
    },
  },
});
