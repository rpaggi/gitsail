<script setup lang="ts">
import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";

import { getRepositoryStatus, openRepository } from "../services/repository";
import { isErrorPayload } from "../services/errors";
import { useRepositorySessionStore } from "../stores/session";
import { useViewStore } from "../stores/view";

const view = useViewStore();
const session = useRepositorySessionStore();
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
  view.lastError = null;
  view.isOpeningRepository = true;
  try {
    session.repository = await openRepository(path.value);
    view.isRefreshingStatus = true;
    try {
      session.status = await getRepositoryStatus();
    } finally {
      view.isRefreshingStatus = false;
    }
  } catch (err) {
    view.lastError = isErrorPayload(err)
      ? err
      : { code: "internal", message: String(err) };
  } finally {
    view.isOpeningRepository = false;
  }
}
</script>

<template>
  <div class="repository-opener">
    <input
      v-model="path"
      type="text"
      placeholder="/path/to/repository"
      :disabled="view.isOpeningRepository"
    />
    <button :disabled="view.isOpeningRepository" @click="pickFolder">Browse…</button>
    <button :disabled="view.isOpeningRepository || !path" @click="openAndRefresh">
      {{ view.isOpeningRepository ? "Opening…" : "Open" }}
    </button>
    <p v-if="view.lastError" class="error">
      {{ view.lastError.message }}
      <span v-if="view.lastError.remediation"> — {{ view.lastError.remediation }}</span>
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
