<script setup lang="ts">
// The generic intent/risk/result dialog T-194/US-061 requires, parametrized
// entirely by `stores/operation.ts`'s `OperationDescriptor` — every story
// that mutates the repository (commit, amend, create/switch/delete branch,
// drag-and-drop) calls `operationStore.request(...)` and this single
// component renders whatever confirmation, progress, or result state that
// produces. Mounted once in `App.vue`; never instantiated per-feature.

import { useOperationStore } from "../stores/operation";

const store = useOperationStore();

function riskLabel(risk: string): string {
  switch (risk) {
    case "safe":
      return "Safe";
    case "moderate":
      return "Moderate";
    case "destructive":
      return "Destructive";
    default:
      return risk;
  }
}
</script>

<template>
  <div v-if="store.status !== 'idle'" class="confirmation-overlay">
    <div
      class="confirmation-dialog"
      :class="`risk-${store.current?.risk}`"
      role="alertdialog"
      aria-modal="true"
    >
      <template v-if="store.status === 'confirming' && store.current">
        <p class="risk-badge">{{ riskLabel(store.current.risk) }}</p>
        <p class="target">{{ store.current.targetLabel }}</p>
        <p v-if="store.current.impact" class="impact">{{ store.current.impact }}</p>
        <div class="actions">
          <button @click="store.cancel()">Cancel</button>
          <button class="confirm" @click="store.confirm()">Confirm</button>
        </div>
      </template>

      <template v-else-if="store.status === 'inProgress' && store.current">
        <p class="target">{{ store.current.targetLabel }}</p>
        <p>Running…</p>
      </template>

      <template v-else-if="store.status === 'succeeded' && store.current">
        <p class="target">{{ store.current.targetLabel }}</p>
        <p class="success">Completed successfully.</p>
        <div class="actions">
          <button @click="store.cancel()">Dismiss</button>
        </div>
      </template>

      <template v-else-if="store.status === 'failed' && store.current">
        <p class="target">{{ store.current.targetLabel }}</p>
        <p class="failure">
          {{ store.error?.message }}
          <span v-if="store.error?.remediation"> — {{ store.error.remediation }}</span>
        </p>
        <div class="actions">
          <button @click="store.cancel()">Dismiss</button>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.confirmation-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.confirmation-dialog {
  background: #2a2a2a;
  border: 1px solid #555;
  border-radius: 4px;
  padding: 1rem 1.25rem;
  min-width: 20rem;
  max-width: 32rem;
}
.confirmation-dialog.risk-destructive {
  border-color: #c0392b;
}
.confirmation-dialog.risk-moderate {
  border-color: #e0a030;
}
.risk-badge {
  display: inline-block;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.8;
  margin: 0 0 0.25rem;
}
.target {
  font-weight: 600;
  margin: 0 0 0.5rem;
}
.impact {
  margin: 0 0 0.75rem;
}
.success {
  color: #2ecc71;
}
.failure {
  color: #e74c3c;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  margin-top: 0.75rem;
}
.confirm {
  font-weight: 600;
}
</style>
