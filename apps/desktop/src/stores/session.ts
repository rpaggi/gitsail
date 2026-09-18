// Repository session state (SAD §17, §21). Frontend mirror of the backend's
// `gitsail_application::RepositorySession`: only what one open repository's
// session holds (repository identity, last status snapshot) — never an
// arbitrary cache of Git data. US-052/US-053 extend this (e.g. a
// `selection` field) rather than inventing a second session-shaped store.

import { defineStore } from "pinia";

import type { RepositoryDto, RepositoryStatusDto } from "../services/dto";

export const useRepositorySessionStore = defineStore("repositorySession", {
  state: () => ({
    repository: null as RepositoryDto | null,
    status: null as RepositoryStatusDto | null,
  }),
});
