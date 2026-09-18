// Copy-or-export-a-patch orchestration (US-029/T-162). Mirrors
// `gitsail-tui`'s `App::export_patch`/`PatchExportOutcome`: copy to the
// clipboard is the primary action, falling back to a file the person picks
// through the native save dialog when the clipboard is unavailable or fails
// (criterion 3) — never silently dropping the patch, and never sending its
// content anywhere but the clipboard or that chosen file (criterion 3's
// "never sent to an external service").
//
// Follows `stores/session.ts`'s state-plus-actions-in-one-store shape.

import { save } from "@tauri-apps/plugin-dialog";
import { defineStore } from "pinia";

import { copyToClipboard } from "../services/clipboard";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { exportPatch as exportPatchCommand, saveTextFile } from "../services/patch";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

/** The last file segment of `path`, used as the save dialog's suggested name. */
function defaultPatchFileName(path: string | null): string {
  const base = path?.split(/[\\/]/).pop() ?? "changes";
  return `${base}.patch`;
}

export type PatchExportOutcome =
  // The patch was copied to the clipboard (the primary action).
  | { kind: "copied"; scope: string; fileCount: number; incomplete: boolean }
  // The clipboard was unavailable/failed, so the patch was saved to `path`
  // instead (criterion 3's documented fallback).
  | { kind: "savedToFile"; scope: string; path: string; incomplete: boolean }
  // The clipboard failed and the person closed the save dialog without
  // choosing a location — the patch was neither copied nor saved, but
  // nothing was lost or sent anywhere either.
  | { kind: "cancelled" }
  // There was nothing to export — every file in scope was binary,
  // truncated, or had no content hunks. Distinct from `failed`: nothing
  // went wrong, there was simply no patchable content.
  | { kind: "empty" }
  | { kind: "failed"; error: ErrorPayload };

export const usePatchExportStore = defineStore("patchExport", {
  state: () => ({
    isExporting: false,
    lastOutcome: null as PatchExportOutcome | null,
  }),
  actions: {
    /**
     * Copies the patch for `staged`/`path`'s diff to the clipboard, or —
     * when the clipboard is unavailable — asks where to save it as a file
     * instead. `scope` is a short, human-readable label the caller already
     * knows from which status row triggered this (US-029 criterion 1:
     * "origem e escopo do patch são informados"); this store never invents
     * one, it only reports it back in `lastOutcome`.
     */
    async exportPatch(staged: boolean, path: string | null, scope: string): Promise<void> {
      this.isExporting = true;
      this.lastOutcome = null;
      try {
        const dto = await exportPatchCommand(staged, path ?? undefined);
        if (dto.patch.length === 0) {
          this.lastOutcome = { kind: "empty" };
          return;
        }
        const incomplete =
          dto.skippedBinaryFiles.length > 0 || dto.skippedTruncatedFiles.length > 0;

        if (await copyToClipboard(dto.patch)) {
          this.lastOutcome = {
            kind: "copied",
            scope,
            fileCount: dto.includedFiles.length,
            incomplete,
          };
          return;
        }

        const savePath = await save({ defaultPath: defaultPatchFileName(path) });
        if (savePath === null) {
          this.lastOutcome = { kind: "cancelled" };
          return;
        }
        await saveTextFile(savePath, dto.patch);
        this.lastOutcome = { kind: "savedToFile", scope, path: savePath, incomplete };
      } catch (error) {
        this.lastOutcome = { kind: "failed", error: toErrorPayload(error) };
      } finally {
        this.isExporting = false;
      }
    },

    /** Dismisses the last outcome banner without exporting again. */
    dismiss(): void {
      this.lastOutcome = null;
    },
  },
});
