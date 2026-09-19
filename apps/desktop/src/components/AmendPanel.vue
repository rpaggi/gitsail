<script setup lang="ts">
// Amend HEAD with confirmation (US-059). Loads a read-only preview
// (HEAD's identity/current message, and the staged diff that would be
// folded in) on demand, then requests the amend through T-194's shared
// confirmation flow, which shows the "this rewrites history" warning text
// `stores/amend.ts` attaches (criterion 2) before anything runs.

import { useAmendStore } from "../stores/amend";
import { useOperationStore } from "../stores/operation";

const amend = useAmendStore();
const operation = useOperationStore();
</script>

<template>
  <div class="amend-panel">
    <button :disabled="amend.isLoadingPreview" @click="amend.loadPreview()">
      {{ amend.preview ? "Refresh preview" : "Amend last commit…" }}
    </button>
    <p v-if="amend.lastError" class="error">{{ amend.lastError.message }}</p>

    <template v-if="amend.preview">
      <p class="amend-panel__head">
        HEAD: <code>{{ amend.preview.head.shortHash }}</code> — {{ amend.preview.head.subject }}
      </p>
      <p class="amend-panel__scope">
        {{ amend.preview.stagedDiff.files.length }} staged file{{
          amend.preview.stagedDiff.files.length === 1 ? "" : "s"
        }}
        will be folded into the amended commit.
      </p>
      <label for="amend-panel-message" class="sr-only">Amended commit message</label>
      <textarea id="amend-panel-message" v-model="amend.message" rows="3" :disabled="operation.isBusy" />
      <button :disabled="amend.message.trim().length === 0 || operation.isBusy" @click="amend.requestAmend()">
        Amend HEAD
      </button>
      <p v-if="amend.lastCommitHash" class="amend-panel__result">
        Amended to {{ amend.lastCommitHash.slice(0, 8) }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.amend-panel {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.amend-panel textarea {
  width: 100%;
  box-sizing: border-box;
  font-family: inherit;
}
.amend-panel__head {
  margin: 0;
}
.amend-panel__scope {
  margin: 0;
  opacity: 0.8;
}
.amend-panel__result {
  margin: 0;
  opacity: 0.8;
}
.error {
  color: var(--color-danger);
}
</style>
