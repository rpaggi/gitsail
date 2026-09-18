// Pull/Merge Request listing state (T-245/US-103), in explicitly limited
// scope — see `gitsail_application::pull_requests`'s module doc for the
// full scope cut (no diffs/comments/CI status; listing only).
//
// `status` is the one field `PullRequestsPanel.vue` switches on to decide
// what to render, and is always exactly one of the explicit states US-103
// criterion 2 requires: `"loading"`, `"noToken"` (folds
// `authenticationRequired`/`permissionDenied` into a single UI state —
// see `load()`'s own comment for why), `"rateLimited"`,
// `"offline"`, `"noForge"`, `"error"`, or `"loaded"` (which itself covers
// both "has PRs/MRs" and "truly empty" — `page!.items.length === 0` is what
// distinguishes them, exactly like every other GitSail list view). Every
// one of these is a distinct value, never inferred from `page` being
// empty/null, so a network failure can never be misread as "no PRs/MRs"
// (the whole point of this criterion).

import { defineStore } from "pinia";

import { listPullRequests, openPullRequestLink } from "../services/pullRequests";
import type { PullRequestPageDto } from "../services/dto";

export type PullRequestsStatus =
  | "idle"
  | "loading"
  | "loaded"
  | "noForge"
  | "noToken"
  | "rateLimited"
  | "offline"
  | "error";

export const usePullRequestsStore = defineStore("pullRequests", {
  state: () => ({
    status: "idle" as PullRequestsStatus,
    page: null as PullRequestPageDto | null,
    currentPage: 1,
    /** Set only when `status === "rateLimited"` and the forge API reported
     * a wait time (US-103 criterion 2: "com tempo de espera se a API
     * informar" — absent entirely when it did not report one, never a
     * fabricated number). */
    retryAfterSeconds: null as number | null,
    /** Set only when `status === "offline"`: a redacted, log-safe
     * diagnostic message (never a raw token — the backend already
     * guarantees this via `redact_secrets`). */
    offlineMessage: null as string | null,
    /** Set only when `status === "error"`: the one truly unexpected
     * failure shape, kept distinct from every other named state. */
    errorMessage: null as string | null,
  }),
  actions: {
    /** Loads `page` (1-based), replacing whatever is currently displayed.
     * Called with `this.currentPage` for "refresh", or `currentPage + 1`/
     * `currentPage - 1` for pagination (see `nextPage`/`previousPage`). */
    async load(page: number): Promise<void> {
      this.status = "loading";
      const outcome = await listPullRequests(page);
      this.currentPage = page;
      switch (outcome.state) {
        case "page":
          this.status = "loaded";
          this.page = outcome.page;
          this.retryAfterSeconds = null;
          this.offlineMessage = null;
          this.errorMessage = null;
          break;
        case "noForgeDetected":
          this.status = "noForge";
          this.page = null;
          break;
        case "authenticationRequired":
        case "permissionDenied":
          // Folded into one UI state deliberately: from the person's point
          // of view, both mean "connect an account (or check the one
          // connected) to see this" — the distinction between "no token"
          // and "token lacks access" is exactly what the backend's own
          // separate `PullRequestQueryError` variants (and their own
          // dedicated Rust tests) exist to verify, but a single "sem
          // token/sem permissão" panel state is all US-103 criterion 2
          // itself asks the UI to show.
          this.status = "noToken";
          this.page = null;
          break;
        case "rateLimited":
          this.status = "rateLimited";
          this.page = null;
          this.retryAfterSeconds = outcome.retryAfterSeconds ?? null;
          break;
        case "offline":
          this.status = "offline";
          this.page = null;
          this.offlineMessage = outcome.message;
          break;
        case "error":
          this.status = "error";
          this.page = null;
          this.errorMessage = outcome.error.message;
          break;
      }
    },

    /** Loads the first page — call this on mount/repository switch. */
    async loadFirstPage(): Promise<void> {
      await this.load(1);
    },

    async nextPage(): Promise<void> {
      if (this.status !== "loaded" || !this.page?.hasNextPage) {
        return;
      }
      await this.load(this.currentPage + 1);
    },

    async previousPage(): Promise<void> {
      if (this.currentPage <= 1) {
        return;
      }
      await this.load(this.currentPage - 1);
    },

    /** Opens a PR/MR's own page in the browser (US-103 criterion 3: always
     * an explicit user action — called only from a click handler). */
    async openInBrowser(url: string): Promise<void> {
      await openPullRequestLink(url);
    },
  },
});
