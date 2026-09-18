<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";

import CommitGraph from "./components/CommitGraph.vue";
import RecentRepositories from "./components/RecentRepositories.vue";
import RepositoryOpener from "./components/RepositoryOpener.vue";
import StatusPanel from "./components/StatusPanel.vue";
import { useAppDataStore } from "./stores/appData";
import { useRepositorySessionStore } from "./stores/session";

const appData = useAppDataStore();
const session = useRepositorySessionStore();

// Refresh-on-window-focus (US-054 criterion 2): regaining focus refreshes
// through the same shared `refreshStatus` action a manual click uses.
// `refreshStatus` itself is a no-op when no repository is open yet, so
// this is safe to register unconditionally.
function handleFocus(): void {
  void session.refreshStatus("focus");
}

onMounted(() => {
  void appData.loadRecentRepositories();
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
    <section class="graph-section">
      <CommitGraph />
    </section>
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
.graph-section {
  height: 60vh;
  margin-top: 1rem;
  border: 1px solid #444;
}
</style>
