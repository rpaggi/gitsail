// Apply a patch, with a non-mutating preview shown before confirmation
// (T-163/US-030). Mirrors `stores/amend.ts`'s own "loadPreview separately,
// then requestX confirms/runs through the generic operation store" shape
// — the inverse of `stores/patchExport.ts` (T-162), which only ever reads
// a diff and never mutates anything.
//
// Risk classification: `Moderate`, matching
// `gitsail_application::mutation::MutationKind::ApplyPatch` and
// `gitsail_tui::operation::OperationKind::ApplyPatch` — applying a patch
// mutates the working tree, but never HEAD/the index, and is not
// irreversible the way a `Destructive` operation is.

import { defineStore } from "pinia";

import { applyPatch, previewPatchApplication } from "../services/patch";
import type { ApplyPatchResultDto, PatchPreviewDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const usePatchApplyStore = defineStore("patchApply", {
  state: () => ({
    /** The patch text a person pasted or loaded from a chosen file —
     * exactly what `loadPreview`/`requestApply` act on, held here (not in
     * the component) so a preview and its later confirmed apply always
     * refer to the same text, never a since-edited one. */
    patchText: "",
    preview: null as PatchPreviewDto | null,
    isLoadingPreview: false,
    /** A hard failure building the preview at all (e.g. no repository
     * open) — distinct from `preview.supported === false`, which is a
     * normal, structured rejection (malformed patch, out-of-repository
     * path, stale context) shown inline rather than as this error. */
    lastError: null as ErrorPayload | null,
    lastResult: null as ApplyPatchResultDto | null,
  }),
  actions: {
    /** Replaces the pending patch text, discarding whatever preview/result
     * referred to the previous text — a stale preview must never be shown
     * (or confirmed) against text the person has since changed. */
    setPatchText(text: string): void {
      this.patchText = text;
      this.preview = null;
      this.lastError = null;
      this.lastResult = null;
    },

    /** Builds the non-mutating preview (`git apply --check`) for the
     * current `patchText` (US-030 criterion 1). A no-op for empty/
     * whitespace-only text — there is nothing to preview. */
    async loadPreview(): Promise<void> {
      if (this.patchText.trim().length === 0) {
        return;
      }
      this.isLoadingPreview = true;
      this.lastError = null;
      try {
        this.preview = await previewPatchApplication(this.patchText);
      } catch (error) {
        this.preview = null;
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoadingPreview = false;
      }
    },

    /** A no-op without a supported, loaded preview — there is nothing
     * confirmable yet (US-030 criterion 2: a rejected/absent preview never
     * reaches confirmation). */
    async requestApply(): Promise<void> {
      const preview = this.preview;
      if (preview === null || !preview.supported) {
        return;
      }
      const patchText = this.patchText;
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      const fileCount = preview.affectedFiles.length;
      await operation.request({
        kind: "applyPatch",
        risk: "moderate",
        promptLabel: `Apply the patch — ${fileCount} file${fileCount === 1 ? "" : "s"} affected`,
        run: async () => {
          const result = await applyPatch(patchText);
          this.lastResult = result;
          this.patchText = "";
          this.preview = null;
          await session.refreshStatus("after_mutation");
        },
      });
    },

    /** Dismisses the last result/error banner without touching the
     * pending patch text. */
    dismiss(): void {
      this.lastError = null;
      this.lastResult = null;
    },
  },
});
