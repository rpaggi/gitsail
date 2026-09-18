<script setup lang="ts">
// File blame overlay (T-195/US-062 criterion 1: "blame de arquivo permite
// relacionar linha e commit") — opened via `stores/blame.ts`'s `open()`
// (see `DiffViewer.vue`'s "Blame" button for the file currently selected
// there). Mounted once, globally, in `App.vue`, alongside
// `ConfirmationDialog.vue`/`HistoryEditingPanel.vue` — the same "app-global
// modal overlay" convention those two already establish, reusing
// `focusTrap.ts` for the same WCAG 2.1.2 focus-trap discipline.
//
// Each line shows its author/date/short hash/content
// (`gitsail_domain::BlameLine` mapped one-to-one, mirroring
// `apps/vscode/src/blameFormat.ts`'s own display convention); clicking a
// committed line's hash fetches and shows that commit's own details
// inline — an uncommitted (`origin: "local"`) line is never offered that
// action, since there is no real commit behind it yet (US-033 criterion 1).

import { nextTick, ref, watch } from "vue";

import { useBlameStore } from "../stores/blame";
import { abbreviateHash, isUncommittedLine } from "./blamePresentation";
import { formatGitTimestamp } from "./timestampFormat";
import { focusFirst, handleFocusTrapKeydown } from "./focusTrap";

const blame = useBlameStore();
const dialogEl = ref<HTMLElement | null>(null);

watch(
  () => blame.file,
  (file) => {
    if (file !== null) {
      void nextTick(() => focusFirst(dialogEl.value));
    }
  },
);

function onDialogKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    blame.close();
    return;
  }
  if (dialogEl.value) {
    handleFocusTrapKeydown(dialogEl.value, event);
  }
}
</script>

<template>
  <div v-if="blame.file !== null" class="blame-overlay">
    <div
      ref="dialogEl"
      class="blame-dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="`Blame for ${blame.file}`"
      @keydown="onDialogKeydown"
    >
      <div class="blame-dialog__header">
        <h2>Blame — {{ blame.file }}</h2>
        <button aria-label="Close blame panel" @click="blame.close()">Close</button>
      </div>

      <p v-if="blame.lastError" class="error" role="alert">{{ blame.lastError.message }}</p>
      <p v-else-if="blame.isLoading" role="status">Loading blame&hellip;</p>
      <p v-else-if="!blame.blame || blame.blame.lines.length === 0">No lines to blame.</p>

      <div v-else class="blame-dialog__body">
        <ul class="blame-dialog__lines">
          <li v-for="line in blame.blame.lines" :key="line.finalLine" class="blame-dialog__line">
            <span class="blame-dialog__lineno">{{ line.finalLine }}</span>
            <template v-if="isUncommittedLine(line)">
              <span class="blame-dialog__meta blame-dialog__meta--uncommitted">Uncommitted change</span>
            </template>
            <template v-else>
              <button
                class="blame-dialog__hash"
                :aria-label="`Open commit ${line.commit} for line ${line.finalLine}`"
                @click="blame.openCommitDetails(line.commit)"
              >
                {{ abbreviateHash(line.commit) }}
              </button>
              <span class="blame-dialog__meta">{{ line.author.name }}, {{ formatGitTimestamp(line.timestamp) }}</span>
            </template>
            <span class="blame-dialog__content">{{ line.content }}</span>
          </li>
        </ul>
      </div>

      <div v-if="blame.isLoadingCommit" class="blame-dialog__commit" role="status">Loading commit&hellip;</div>
      <div v-else-if="blame.commitError" class="blame-dialog__commit error" role="alert">
        {{ blame.commitError.message }}
        <button @click="blame.dismissCommitDetails()">Dismiss</button>
      </div>
      <div v-else-if="blame.selectedCommit" class="blame-dialog__commit">
        <h3>Commit {{ abbreviateHash(blame.selectedCommit.hash) }}</h3>
        <p>{{ blame.selectedCommit.author.name }} &lt;{{ blame.selectedCommit.author.email }}&gt;</p>
        <p>{{ formatGitTimestamp(blame.selectedCommit.authorDate) }}</p>
        <p class="blame-dialog__subject">{{ blame.selectedCommit.subject }}</p>
        <button @click="blame.dismissCommitDetails()">Close commit details</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.blame-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.blame-dialog {
  background: var(--color-surface, #2a2a2a);
  border: 1px solid var(--color-border, #555);
  border-radius: 4px;
  padding: 1rem 1.25rem;
  width: 42rem;
  max-width: 92vw;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.blame-dialog__header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.blame-dialog__header h2 {
  margin: 0;
  font-size: 1rem;
}
.blame-dialog__body {
  overflow-y: auto;
  font-family: monospace;
  font-size: 0.8rem;
}
.blame-dialog__lines {
  list-style: none;
  margin: 0;
  padding: 0;
}
.blame-dialog__line {
  display: flex;
  gap: 0.5rem;
  align-items: baseline;
  padding: 0.1rem 0;
  border-bottom: 1px solid var(--color-border, #444);
  white-space: pre;
}
.blame-dialog__lineno {
  width: 3rem;
  flex-shrink: 0;
  opacity: 0.6;
  text-align: right;
}
.blame-dialog__hash {
  background: none;
  border: none;
  color: var(--color-accent, #6cb2ff);
  cursor: pointer;
  padding: 0;
  font-family: monospace;
  text-decoration: underline;
}
.blame-dialog__meta {
  flex-shrink: 0;
  opacity: 0.75;
}
.blame-dialog__meta--uncommitted {
  font-style: italic;
}
.blame-dialog__content {
  overflow: hidden;
  text-overflow: ellipsis;
}
.blame-dialog__commit {
  border-top: 1px solid var(--color-border, #555);
  padding-top: 0.5rem;
}
.blame-dialog__commit p {
  margin: 0.1rem 0;
}
.blame-dialog__subject {
  font-weight: 600;
}
.error {
  color: var(--color-danger, #c0392b);
}
</style>
