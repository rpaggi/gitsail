// Persisted preferences state (SAD §17). Placeholder in this story — no
// persistence (e.g. `tauri-plugin-store`) is wired up yet, that's follow-up
// work — but the module boundary exists now so US-053 (layout/theme
// preferences, currently blocked on US-129) has a designated store rather
// than preferences being invented ad hoc later.

import { defineStore } from "pinia";

export const usePreferencesStore = defineStore("preferences", {
  state: () => ({}),
});
