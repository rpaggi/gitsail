<script setup lang="ts">
import { useAppDataStore } from "../stores/appData";
import { useRepositorySessionStore } from "../stores/session";

const appData = useAppDataStore();
const session = useRepositorySessionStore();

function formatLastOpened(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString();
}
</script>

<template>
  <div class="recent-repositories">
    <h2>Recent repositories</h2>
    <p v-if="appData.isLoadingRecents">Loading…</p>
    <p v-else-if="appData.recentRepositories.length === 0">No recent repositories yet.</p>
    <ul v-else>
      <li v-for="entry in appData.recentRepositories" :key="entry.path">
        <template v-if="appData.invalidRecents[entry.path]">
          <span class="error">
            {{ entry.path }} — {{ appData.invalidRecents[entry.path].message }}
          </span>
          <button @click="appData.forgetRecentRepository(entry.path)">Remove from list</button>
        </template>
        <template v-else>
          <button
            :disabled="session.isOpening"
            @click="appData.openRecentRepository(entry.path)"
          >
            {{ entry.path }}
          </button>
          <span class="last-opened">{{ formatLastOpened(entry.lastOpenedUnixSeconds) }}</span>
        </template>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.recent-repositories ul {
  list-style: none;
  padding: 0;
}
.recent-repositories li {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
.error {
  color: var(--color-danger);
}
.last-opened {
  opacity: 0.7;
  font-size: 0.85em;
}
</style>
