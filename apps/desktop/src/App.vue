<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";

import AppShell from "./components/AppShell.vue";
import BlamePanel from "./components/BlamePanel.vue";
import ConfirmationDialog from "./components/ConfirmationDialog.vue";
import HistoryEditingPanel from "./components/HistoryEditingPanel.vue";
import { useAppDataStore } from "./stores/appData";
import { useCommitGraphStore } from "./stores/graph";
import { useKeybindingsStore } from "./stores/keybindings";
import { usePreferencesStore } from "./stores/preferences";
import { useRepositorySessionStore } from "./stores/session";
import { takeStartupIntent } from "./services/startup";

const appData = useAppDataStore();
const session = useRepositorySessionStore();
const preferences = usePreferencesStore();
const keybindings = useKeybindingsStore();

// Refresh-on-window-focus (US-054 criterion 2): regaining focus refreshes
// through the same shared `refreshStatus` action a manual click uses.
// `refreshStatus` itself is a no-op when no repository is open yet, so
// this is safe to register unconditionally. `refreshStatus` itself also
// re-detects the in-progress operation for a `"focus"` reason (T-234/
// US-082 criterion 2) — see `stores/session.ts` — so this one call already
// covers both.
function handleFocus(): void {
  void session.refreshStatus("focus");
}

/**
 * Applies the startup handoff (closing the EPIC-15 gap: VS Code's
 * T-210/US-077 launches this process with `--repo <path> --commit
 * <hash>`). Consumed exactly once via `take_startup_intent` — a re-mount
 * of this component (hot reload, etc.) gets an empty intent the second
 * time, so it never re-opens/re-selects the same target again.
 */
async function applyStartupIntent(): Promise<void> {
  const intent = await takeStartupIntent();
  if (!intent.repoPath) {
    return;
  }
  const result = await session.openRepository(intent.repoPath);
  if (result.status !== "opened") {
    return;
  }
  await appData.loadRecentRepositories();
  if (intent.commitHash) {
    // Shares selection identity with every other selector (US-056
    // criterion 2) — the graph highlights it once its page has loaded.
    useCommitGraphStore().select(intent.commitHash);
  }
}

onMounted(() => {
  void appData.loadRecentRepositories();
  // T-248/US-106, T-249/US-107: app-global preferences, loaded once
  // regardless of whether a repository is open — `preferences.load()`
  // applies the persisted (or default-dark) theme to the document as soon
  // as it resolves; `keybindings.load()` populates the overrides the
  // global shortcut dispatcher (`AppShell.vue`) and `SearchPalette.vue`'s
  // shortcut hint both read.
  void preferences.load();
  void keybindings.load();
  void applyStartupIntent();
  window.addEventListener("focus", handleFocus);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", handleFocus);
});
</script>

<template>
  <div id="gitsail-app">
    <!--
      `AppShell.vue` (T-186/US-053) owns the actual page layout — header
      with the GitSail identity, sidebar, and the tabbed main workspace —
      and is this app's single `<main>` landmark. `ConfirmationDialog`/
      `HistoryEditingPanel`/`BlamePanel` stay mounted here, once, as
      app-global modal overlays (T-194/US-061, T-240/US-088, T-195/US-062):
      they render above *everything* AppShell contains regardless of which
      sidebar section or tab is active, so they belong beside it, not
      nested inside one of its regions.
    -->
    <AppShell />
    <ConfirmationDialog />
    <HistoryEditingPanel />
    <BlamePanel />
  </div>
</template>
