// Tags/stash read state (EPIC-18/US-091, EPIC-12/T-195/US-062). Purely a
// read model — there is no mutation here, matching `RepositoryReadPort::
// list_tags`/`list_stash_entries`'s own read-only contract — so, unlike
// `stores/branches.ts`/`stores/sync.ts`, nothing here goes through
// `stores/operation.ts`'s confirm/run flow. Remotes are deliberately not
// duplicated in this store: `stores/sync.ts`'s own `remotes` field is the
// single source of truth `components/ReferencesPanel.vue` reads for its
// Remotes section.

import { defineStore } from "pinia";

import { listStashEntries, listTags } from "../services/references";
import type { StashDto, TagDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useReferencesStore = defineStore("references", {
  state: () => ({
    tags: [] as TagDto[],
    isLoadingTags: false,
    tagsError: null as ErrorPayload | null,

    stashes: [] as StashDto[],
    isLoadingStashes: false,
    stashesError: null as ErrorPayload | null,
  }),
  actions: {
    async loadTags(): Promise<void> {
      this.isLoadingTags = true;
      try {
        this.tags = await listTags();
        this.tagsError = null;
      } catch (error) {
        this.tagsError = toErrorPayload(error);
      } finally {
        this.isLoadingTags = false;
      }
    },

    async loadStashes(): Promise<void> {
      this.isLoadingStashes = true;
      try {
        this.stashes = await listStashEntries();
        this.stashesError = null;
      } catch (error) {
        this.stashesError = toErrorPayload(error);
      } finally {
        this.isLoadingStashes = false;
      }
    },

    /** Loads both lists — the convenience `ReferencesPanel.vue` calls on
     * mount, mirroring `SyncPanel.vue`'s own `onMounted` pattern. */
    async loadAll(): Promise<void> {
      await Promise.all([this.loadTags(), this.loadStashes()]);
    },
  },
});
