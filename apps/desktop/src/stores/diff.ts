// Diff view state (US-057). Holds exactly one loaded `DiffDto` plus the
// view mode toggle — switching `mode` never re-fetches or otherwise
// touches `file`/`staged`/`diff` (criterion 1: toggling unified/side-by-
// side preserves file and staged/unstaged side). Presentation math
// (unified line numbering, side-by-side pairing) lives in
// `components/diffPresentation.ts`, never recomputed here.

import { defineStore } from "pinia";

import { getDiff } from "../services/diff";
import type { DiffDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

export type DiffViewMode = "unified" | "sideBySide";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useDiffStore = defineStore("diff", {
  state: () => ({
    file: null as string | null,
    staged: false,
    mode: "unified" as DiffViewMode,
    diff: null as DiffDto | null,
    isLoading: false,
    lastError: null as ErrorPayload | null,
    /** A staleness token, mirroring `stores/session.ts`'s `generation`:
     * bumped on every `load()` call so an older, still-in-flight load can
     * never overwrite a newer one's result once both resolve. */
    requestId: 0,
  }),
  actions: {
    async load(file: string | null, staged: boolean): Promise<void> {
      const requestId = ++this.requestId;
      this.isLoading = true;
      try {
        const diff = await getDiff({ staged, path: file ?? undefined });
        if (requestId !== this.requestId) {
          return; // superseded by a newer load; discard this stale result
        }
        this.file = file;
        this.staged = staged;
        this.diff = diff;
        this.lastError = null;
      } catch (error) {
        if (requestId !== this.requestId) {
          return;
        }
        this.lastError = toErrorPayload(error);
      } finally {
        if (requestId === this.requestId) {
          this.isLoading = false;
        }
      }
    },

    setMode(mode: DiffViewMode): void {
      this.mode = mode;
    },

    toggleMode(): void {
      this.mode = this.mode === "unified" ? "sideBySide" : "unified";
    },
  },
});
