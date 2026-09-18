// Application data state (SAD §17). Data that outlives a single repository
// session but is not a persisted user preference — e.g. the future
// recent-repositories list (US-052). Empty placeholder in this story: its
// purpose is to give US-052 a designated home instead of that list landing
// in the session or view store by default.

import { defineStore } from "pinia";

export const useAppDataStore = defineStore("appData", {
  state: () => ({}),
});
