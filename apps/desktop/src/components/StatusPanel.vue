<script setup lang="ts">
import { useRepositorySessionStore } from "../stores/session";
import { useViewStore } from "../stores/view";

const session = useRepositorySessionStore();
const view = useViewStore();
</script>

<template>
  <div class="status-panel">
    <p v-if="view.isRefreshingStatus">Refreshing status…</p>
    <template v-else-if="session.repository && session.status">
      <p>
        <strong>{{ session.repository.currentBranch ?? "(detached)" }}</strong>
        — {{ session.status.isClean ? "clean" : "dirty" }}
      </p>
      <ul v-if="!session.status.isClean">
        <li v-for="file in session.status.files" :key="file.path">
          {{ file.worktreeStatus }} {{ file.path }}
        </li>
      </ul>
    </template>
    <p v-else>No repository open.</p>
  </div>
</template>
