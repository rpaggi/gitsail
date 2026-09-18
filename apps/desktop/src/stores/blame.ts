// File blame state (EPIC-07/US-031..034, EPIC-12/T-195/US-062 criterion 1:
// "blame de arquivo permite relacionar linha e commit"). Purely a read
// model, like `stores/references.ts` — blame never mutates the repository,
// so there is no `stores/operation.ts` confirm/run flow here.
//
// `selectedCommit` is the "open the corresponding commit's details"
// half of criterion 1: clicking a blame line's hash fetches that commit via
// the same `get_commit` command `stores/search.ts` already uses (US-056),
// so a blame line and a search result resolve to identical commit detail
// data rather than a second, bespoke shape.

import { defineStore } from "pinia";

import { getBlame } from "../services/blame";
import { getCommit } from "../services/search";
import type { BlameDto, CommitDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useBlameStore = defineStore("blame", {
  state: () => ({
    /** The file currently open for blame, or `null` when the panel is
     * closed (`components/DiffViewer.vue`'s "Blame" button both opens this
     * panel and sets this). */
    file: null as string | null,
    blame: null as BlameDto | null,
    isLoading: false,
    lastError: null as ErrorPayload | null,

    /** The commit fetched for whichever blame line's hash was last opened
     * (criterion 1's "abrir os detalhes do commit correspondente") —
     * `null` until a line is opened, and cleared whenever the blame panel
     * itself closes so a stale commit is never shown against a different
     * file's blame. */
    selectedCommit: null as CommitDto | null,
    isLoadingCommit: false,
    commitError: null as ErrorPayload | null,

    /** A staleness token, mirroring `stores/diff.ts`'s own `requestId`: an
     * older, still-in-flight `open` call can never overwrite a newer one's
     * result once both resolve (e.g. blaming a second file before the
     * first file's query returned). */
    requestId: 0,
  }),
  actions: {
    /** Opens the blame panel for `path` (US-031/US-033 criterion 1's
     * default: the working tree, including uncommitted changes). Clears
     * any previously selected commit — it belonged to whichever file/line
     * was open before. */
    async open(path: string, revision?: string): Promise<void> {
      const requestId = ++this.requestId;
      this.file = path;
      this.isLoading = true;
      this.selectedCommit = null;
      this.commitError = null;
      try {
        const blame = await getBlame({ path, revision });
        if (requestId !== this.requestId) {
          return; // superseded by a newer open(); discard this stale result
        }
        this.blame = blame;
        this.lastError = null;
      } catch (error) {
        if (requestId !== this.requestId) {
          return;
        }
        this.blame = null;
        this.lastError = toErrorPayload(error);
      } finally {
        if (requestId === this.requestId) {
          this.isLoading = false;
        }
      }
    },

    /** Closes the blame panel, discarding any in-flight `open()`/
     * `openCommitDetails()` result once it resolves. */
    close(): void {
      ++this.requestId;
      this.file = null;
      this.blame = null;
      this.lastError = null;
      this.isLoading = false;
      this.selectedCommit = null;
      this.commitError = null;
      this.isLoadingCommit = false;
    },

    /** Fetches and shows `hash`'s commit details (criterion 1: relating a
     * blame line to the commit that introduced it). A caller must never
     * call this for a `"local"`-origin line (Git's own "not committed yet"
     * attribution, US-033) — there is no real commit to open for one. */
    async openCommitDetails(hash: string): Promise<void> {
      const requestId = this.requestId;
      this.isLoadingCommit = true;
      try {
        const commit = await getCommit(hash);
        if (requestId !== this.requestId) {
          return; // the blame panel was closed/reopened meanwhile
        }
        this.selectedCommit = commit;
        this.commitError = null;
      } catch (error) {
        if (requestId !== this.requestId) {
          return;
        }
        this.selectedCommit = null;
        this.commitError = toErrorPayload(error);
      } finally {
        if (requestId === this.requestId) {
          this.isLoadingCommit = false;
        }
      }
    },

    dismissCommitDetails(): void {
      this.selectedCommit = null;
      this.commitError = null;
    },
  },
});
