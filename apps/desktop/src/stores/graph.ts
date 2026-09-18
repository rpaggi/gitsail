// Commit graph state (US-067). Distinct from `session.ts`'s repository
// session (SAD §21: HEAD/status/selection): this store only ever holds
// what has been paginated in from `get_commit_graph_page` plus this
// window's own selection/hover — never a second copy of the layout the
// Core already computed (rows/edges/laneCount are used exactly as
// received, never recomputed here).

import { defineStore } from "pinia";

import { getCommitGraphPage } from "../services/graph";
import type { CommitGraphRowDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useCommitGraphStore = defineStore("commitGraph", {
  state: () => ({
    rows: [] as CommitGraphRowDto[],
    laneCount: 0,
    hasMore: false,
    nextCursor: null as string | null,
    branchFilter: null as string | null,
    selectedHash: null as string | null,
    hoverHash: null as string | null,
    isLoading: false,
    lastError: null as ErrorPayload | null,
  }),
  getters: {
    // The row index for a commit hash, or `-1` if it has not loaded
    // (yet) — stable across `loadMore` the same way
    // `gitsail_domain::CommitGraph::row_index_of` is on the Core side,
    // since `loadMore` only ever appends to `rows` (US-067 criterion 3).
    rowIndexOf: (state) => (hash: string): number =>
      state.rows.findIndex((row) => row.commit.hash === hash),
  },
  actions: {
    // Starts a fresh graph — the first page for the current (or a new)
    // branch filter. Any previously loaded rows are discarded, mirroring
    // `reset: true` resetting the backend's own accumulated
    // `CommitGraph` (US-067 criterion 3): the two never disagree about
    // what "the graph" currently means.
    async loadFirstPage(branch: string | null = null): Promise<void> {
      this.isLoading = true;
      try {
        const page = await getCommitGraphPage({
          branch: branch ?? undefined,
          reset: true,
        });
        this.rows = page.rows;
        this.laneCount = page.laneCount;
        this.hasMore = page.hasMore;
        this.nextCursor = page.nextCursor ?? null;
        this.branchFilter = branch;
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
      }
    },

    // Appends the next page to the already-loaded rows. A no-op while a
    // load is already in flight or the last page reported no more
    // history, so scrolling repeatedly (or clicking "load more" more than
    // once) can never issue overlapping requests.
    async loadMore(): Promise<void> {
      if (this.isLoading || !this.hasMore) {
        return;
      }
      this.isLoading = true;
      try {
        const page = await getCommitGraphPage({
          branch: this.branchFilter ?? undefined,
          cursor: this.nextCursor ?? undefined,
          reset: false,
        });
        this.rows = this.rows.concat(page.rows);
        this.laneCount = Math.max(this.laneCount, page.laneCount);
        this.hasMore = page.hasMore;
        this.nextCursor = page.nextCursor ?? null;
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
      }
    },

    select(hash: string | null): void {
      this.selectedHash = hash;
    },

    hover(hash: string | null): void {
      this.hoverHash = hash;
    },
  },
});
