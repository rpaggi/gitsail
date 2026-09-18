// Desktop update checking (T-260/US-127), following `preferences.ts`'s own
// convention: a thin Pinia wrapper over the typed service
// (`../services/preferences`'s `checkForUpdate`/`setCheckForUpdates`/
// `openUpdateLink`), never calling `invoke` directly.
//
// Never an auto-installer — see `gitsail_application::update_check`'s own
// module doc comment (mirrored on the Rust side of this feature) for the
// full "check-only, ADR-023 requires this" rationale. This store only ever
// asks "is there a newer release", shows the answer, and hands the person
// a link to open themselves; nothing here ever downloads or installs
// anything.

import { defineStore } from "pinia";

import { checkForUpdate as checkForUpdateCommand, openUpdateLink } from "../services/preferences";
import type { UpdateCheckOutcomeDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useUpdateStore = defineStore("update", {
  state: () => ({
    outcome: null as UpdateCheckOutcomeDto | null,
    isChecking: false,
    /** Set only when the `check_for_update` command itself could not be
     * reached at all (e.g. Tauri's own IPC failed) — distinct from
     * `outcome.state === "checkFailed"`, which is this feature's own
     * "network/malformed response" state and is not an error. This field
     * should, in practice, stay `null`: a normal offline/timeout is
     * already `outcome`, never a thrown error. */
    lastError: null as ErrorPayload | null,
  }),
  getters: {
    /** Whether a newer release exists — the one state that should draw
     * the person's attention (a banner, a badge, ...). Every other state
     * (up to date, skipped, checking failed, no releases yet) is quiet by
     * design (US-127 criterion 2: a failed/skipped check must never look
     * broken or demand attention). */
    hasUpdate(state): boolean {
      return state.outcome?.state === "updateAvailable";
    },
  },
  actions: {
    /**
     * Runs a check. `trigger: "automatic"` (app-mount) is throttled and
     * disableable by the backend itself (`gitsail_application::
     * update_check`'s own gate — this store does not duplicate that
     * logic); `"manual"` (an explicit "check for updates now" click)
     * always runs. Never throws: a network/malformed-response failure
     * lands in `outcome` as `"checkFailed"`, not `lastError` (see this
     * store's own `lastError` doc comment) — the person can always dismiss
     * or retry, this never leaves the app looking broken (US-127
     * criterion 2).
     */
    async check(trigger: "automatic" | "manual"): Promise<void> {
      this.isChecking = true;
      try {
        this.outcome = await checkForUpdateCommand(trigger);
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isChecking = false;
      }
    },

    /** Opens the release page or its `SHA256SUMS.txt` — the only two links
     * this feature ever surfaces (see `openUpdateLink`'s own doc comment
     * for the host re-validation the backend applies before opening
     * anything). */
    async openLink(url: string): Promise<void> {
      try {
        await openUpdateLink(url);
      } catch (error) {
        this.lastError = toErrorPayload(error);
      }
    },
  },
});
