<script setup lang="ts">
import { useRepositorySessionStore } from "../stores/session";

const session = useRepositorySessionStore();

function refresh(): void {
  // The manual refresh trigger (US-054 criterion 2) — the same shared
  // `refreshStatus` action a window-focus event and a future
  // post-mutation refresh also call.
  void session.refreshStatus("manual");
}
</script>

<template>
  <div class="status-panel">
    <template v-if="session.repository">
      <p>
        <strong>{{ session.repository.currentBranch ?? "(detached)" }}</strong>
        <template v-if="session.status">
          — {{ session.status.isClean ? "clean" : "dirty" }}
        </template>
        <button :disabled="session.isRefreshing" @click="refresh">
          {{ session.isRefreshing ? "Refreshing…" : "Refresh" }}
        </button>
      </p>
      <ul v-if="session.status && !session.status.isClean">
        <li v-for="file in session.status.files" :key="file.path">
          {{ file.worktreeStatus }} {{ file.path }}
        </li>
      </ul>
    </template>
    <p v-else>No repository open.</p>
  </div>
</template>
