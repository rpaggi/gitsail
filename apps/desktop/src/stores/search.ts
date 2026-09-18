// Global commit/branch search (US-056). Reuses exactly the Core's
// `CommitQuery` filters (`searchCommits`, `services/search.ts`) rather than
// a second query shape — hash, message (`textQuery`), author, and branch
// are all covered; tag search is out of scope (documented gap: the Core
// read port has no "list every tag" capability yet, EPIC-18).
//
// Selection identity (criterion 2: "graph/lista/detalhes preservam
// identidade da seleção") is never duplicated here: choosing a result
// calls straight into `stores/graph.ts`'s own `select`, the same store
// `CommitGraph.vue` already reads from — this store only ever holds
// *search* state (the query text and its results), never a second copy of
// "what is selected".

import { defineStore } from "pinia";

import { getCommit, looksLikeCommitHash, searchCommits } from "../services/search";
import { listBranches } from "../services/branches";
import type { BranchDto, CommitDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useCommitGraphStore } from "./graph";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useSearchStore = defineStore("search", {
  state: () => ({
    query: "",
    commitResults: [] as CommitDto[],
    branchResults: [] as BranchDto[],
    /** An exact commit match when `query` looks like a hash and resolves
     * to a real commit — shown ahead of the text-query results, since a
     * hash search is unambiguous where a substring match is not. */
    exactMatch: null as CommitDto | null,
    isLoading: false,
    lastError: null as ErrorPayload | null,
  }),
  actions: {
    /** Runs a search for `query` against commit history (hash/message/
     * author) and branch names (US-056 criterion 1). A blank query clears
     * every result without issuing a request. */
    async search(query: string): Promise<void> {
      this.query = query;
      const trimmed = query.trim();
      if (trimmed.length === 0) {
        this.commitResults = [];
        this.branchResults = [];
        this.exactMatch = null;
        this.lastError = null;
        return;
      }

      this.isLoading = true;
      try {
        const [commits, branches, exactMatch] = await Promise.all([
          searchCommits({ textQuery: trimmed }),
          listBranches(),
          looksLikeCommitHash(trimmed) ? getCommit(trimmed).catch(() => null) : Promise.resolve(null),
        ]);
        this.commitResults = commits;
        this.branchResults = branches.filter((b) =>
          b.name.toLowerCase().includes(trimmed.toLowerCase()),
        );
        this.exactMatch = exactMatch;
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
      }
    },

    /** Selects a commit result — shares identity with the graph/details
     * panels via `stores/graph.ts`, never a separate selection state. */
    selectCommit(hash: string): void {
      useCommitGraphStore().select(hash);
    },

    clear(): void {
      this.query = "";
      this.commitResults = [];
      this.branchResults = [];
      this.exactMatch = null;
      this.lastError = null;
    },
  },
});
