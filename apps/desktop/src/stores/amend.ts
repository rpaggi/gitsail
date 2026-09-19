// Amend HEAD with confirmation (US-059). `loadPreview` is read-only
// (never touches the index, working tree, or HEAD) and pre-fills the
// editable message from HEAD's current one (criterion 1). `requestAmend`
// echoes back exactly the hash `loadPreview` last saw as HEAD —
// `AmendCommit`/`RepositoryWritePort::amend_commit` on the Core side
// revalidate it is still HEAD immediately before amending and refuse
// (`operation_conflict`) otherwise (criterion 3), so a stale preview can
// never rewrite the wrong commit.
//
// Risk classification: `gitsail_tui::operation::OperationKind` has no
// amend variant yet (amend does not exist in the TUI), so there is no
// existing Core/TUI precedent to mirror here. This store classifies amend
// as **Destructive** rather than Moderate (unlike a plain `createCommit`):
// unlike an ordinary commit, amend replaces a commit that may already be
// shared/pushed, and the confirmation text says so explicitly (US-059
// criterion 2's "texto real de aviso, não genérico").

import { defineStore } from "pinia";

import { amendCommit, previewAmend } from "../services/amend";
import type { AmendPreviewDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useAmendStore = defineStore("amend", {
  state: () => ({
    preview: null as AmendPreviewDto | null,
    message: "",
    isLoadingPreview: false,
    lastError: null as ErrorPayload | null,
    lastCommitHash: null as string | null,
  }),
  actions: {
    async loadPreview(): Promise<void> {
      this.isLoadingPreview = true;
      try {
        this.preview = await previewAmend();
        this.message = this.preview.head.body
          ? `${this.preview.head.subject}\n\n${this.preview.head.body}`
          : this.preview.head.subject;
        this.lastError = null;
      } catch (error) {
        this.preview = null;
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoadingPreview = false;
      }
    },

    /** A no-op without a loaded preview — there is nothing to amend, and
     * echoing back an `expectedHead` this store never actually saw would
     * defeat the whole point of the revalidation. */
    async requestAmend(): Promise<void> {
      const preview = this.preview;
      if (preview === null) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "amendCommit",
        risk: "destructive",
        promptLabel: `Amend HEAD (${preview.head.shortHash})`,
        impact:
          "This replaces the last commit with a new one carrying the message below. " +
          "If this commit has already been pushed or shared, rewriting it means anyone " +
          "who already has the old one will need to rebase or reset onto the new commit.",
        run: async () => {
          const result = await amendCommit(this.message, preview.head.hash);
          this.lastCommitHash = result.hash;
          this.preview = null;
          await session.refreshStatus("after_mutation");
        },
      });
    },
  },
});
