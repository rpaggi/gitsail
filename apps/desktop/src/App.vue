<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";

import AmendPanel from "./components/AmendPanel.vue";
import BranchPanel from "./components/BranchPanel.vue";
import CommitGraph from "./components/CommitGraph.vue";
import ConfirmationDialog from "./components/ConfirmationDialog.vue";
import DiffViewer from "./components/DiffViewer.vue";
import RecentRepositories from "./components/RecentRepositories.vue";
import RepositoryOpener from "./components/RepositoryOpener.vue";
import SearchPalette from "./components/SearchPalette.vue";
import StagingPanel from "./components/StagingPanel.vue";
import StatusPanel from "./components/StatusPanel.vue";
import SyncPanel from "./components/SyncPanel.vue";
import { useAppDataStore } from "./stores/appData";
import { useCommitGraphStore } from "./stores/graph";
import { useRepositorySessionStore } from "./stores/session";
import { takeStartupIntent } from "./services/startup";

const appData = useAppDataStore();
const session = useRepositorySessionStore();

// Refresh-on-window-focus (US-054 criterion 2): regaining focus refreshes
// through the same shared `refreshStatus` action a manual click uses.
// `refreshStatus` itself is a no-op when no repository is open yet, so
// this is safe to register unconditionally.
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
  void applyStartupIntent();
  window.addEventListener("focus", handleFocus);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", handleFocus);
});
</script>

<template>
  <main>
    <h1>GitSail</h1>
    <RepositoryOpener />
    <RecentRepositories />
    <StatusPanel />
    <div class="workspace">
      <aside class="workspace__sidebar">
        <SearchPalette />
        <BranchPanel />
        <SyncPanel />
        <AmendPanel />
      </aside>
      <div class="workspace__main">
        <section class="graph-section">
          <CommitGraph />
        </section>
        <StagingPanel />
        <DiffViewer />
      </div>
    </div>
    <ConfirmationDialog />
  </main>
</template>

<style>
body {
  margin: 0;
  font-family: system-ui, sans-serif;
  background: #1e1e1e;
  color: #e0e0e0;
}
main {
  padding: 1.5rem;
}
.workspace {
  display: flex;
  gap: 1rem;
  margin-top: 1rem;
}
.workspace__sidebar {
  width: 20rem;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.workspace__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.graph-section {
  height: 40vh;
  border: 1px solid #444;
}
</style>
