<script setup lang="ts">
import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";

import { useAppDataStore } from "../stores/appData";
import { useRepositorySessionStore } from "../stores/session";

const session = useRepositorySessionStore();
const appData = useAppDataStore();
const path = ref("");

async function pickFolder(): Promise<void> {
  const selected = await open({ directory: true, multiple: false });
  if (typeof selected === "string") {
    path.value = selected;
  }
}

async function openAndRefresh(): Promise<void> {
  if (!path.value) {
    return;
  }
  const result = await session.openRepository(path.value);
  if (result.status === "opened") {
    // Opening also records/re-orders this path as a recent on the
    // backend (US-052 criterion 1); keep the recents list in sync.
    await appData.loadRecentRepositories();
  }
}
</script>

<template>
  <div class="repository-opener">
    <label for="repository-opener-path" class="sr-only">Repository path</label>
    <input
      id="repository-opener-path"
      v-model="path"
      type="text"
      placeholder="/path/to/repository"
      :disabled="session.isOpening"
    />
    <button :disabled="session.isOpening" @click="pickFolder">Browse…</button>
    <button :disabled="session.isOpening || !path" @click="openAndRefresh">
      {{ session.isOpening ? "Opening…" : "Open" }}
    </button>
    <p v-if="session.lastError" class="error">
      {{ session.lastError.message }}
      <span v-if="session.lastError.remediation"> — {{ session.lastError.remediation }}</span>
    </p>
  </div>
</template>

<style scoped>
.repository-opener {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
.error {
  color: #c0392b;
  margin: 0;
}
</style>
