// Application data state (SAD §17): the recent-repositories list (US-052),
// the home this store was reserved for when it shipped as an empty
// placeholder in T-184/US-051.
//
// `invalidRecents` tracks, per path, the error a failed "open this recent"
// attempt reported (e.g. the repository was moved or deleted). US-052
// criterion 2 requires that a moved/inaccessible entry show a recovery
// state *without* disappearing from the list on its own — so a failure
// here only ever marks the entry, it never removes it. Removal happens
// only through `forgetRecentRepository`, the explicit user confirmation.

import { defineStore } from "pinia";

import { forgetRecentRepository as forgetRecentRepositoryCommand, listRecentRepositories } from "../services/recentRepositories";
import type { RecentRepositoryDto } from "../services/dto";
import type { ErrorPayload } from "../services/errors";
import { useRepositorySessionStore } from "./session";

export const useAppDataStore = defineStore("appData", {
  state: () => ({
    recentRepositories: [] as RecentRepositoryDto[],
    invalidRecents: {} as Record<string, ErrorPayload>,
    isLoadingRecents: false,
  }),
  actions: {
    /**
     * Loads the recent-repositories list from the backend (US-052
     * criterion 1). Best-effort: an unreadable recents file must never
     * block using the app to open a repository directly — a failure here
     * just leaves the list empty rather than surfacing as a hard error.
     */
    async loadRecentRepositories(): Promise<void> {
      this.isLoadingRecents = true;
      try {
        this.recentRepositories = await listRecentRepositories();
      } catch {
        this.recentRepositories = [];
      } finally {
        this.isLoadingRecents = false;
      }
    },

    /**
     * Opens a recent entry through the shared repository session — the
     * same `openRepository` action `RepositoryOpener.vue` uses, so
     * generation isolation (US-052 criterion 3) is enforced identically
     * regardless of how the open was triggered.
     *
     * On failure — e.g. the repository was moved or deleted — the entry
     * is kept in `recentRepositories` and marked in `invalidRecents`
     * instead of being removed (US-052 criterion 2). A successful open
     * clears any earlier invalid mark and reloads the list, since opening
     * also re-records/re-orders this entry on the backend.
     */
    async openRecentRepository(path: string): Promise<void> {
      const session = useRepositorySessionStore();
      delete this.invalidRecents[path];

      const result = await session.openRepository(path);
      if (result.status === "failed") {
        this.invalidRecents[path] = result.error;
      } else if (result.status === "opened") {
        await this.loadRecentRepositories();
      }
    },

    /**
     * Removes `path` from the recent-repositories list — the explicit
     * confirmation US-052 criterion 2 requires before a moved/inaccessible
     * entry disappears.
     */
    async forgetRecentRepository(path: string): Promise<void> {
      try {
        this.recentRepositories = await forgetRecentRepositoryCommand(path);
      } finally {
        delete this.invalidRecents[path];
      }
    },
  },
});
