<script setup lang="ts">
// The reset-mode chooser (T-240/US-088), the one piece of T-238/T-239/
// T-240 (cherry-pick, revert, reset) that needs UI beyond a single request/
// confirm pair: soft/mixed/hard each have a distinct, concrete effect on
// HEAD/the index/the working tree (criterion 1), and `hard` additionally
// needs the live predicted-loss count shown *before* the shared
// `ConfirmationDialog.vue`'s own reinforced confirmation step (criterion 2).
// Cherry-pick and revert need no panel of their own beyond that shared
// dialog — they are wired directly from `CommitGraph.vue`'s context menu
// into `stores/merge.ts`'s `requestCherryPick`/`requestRevert`, mirroring
// how a merge/rebase target is chosen from `BranchPanel.vue`'s own list
// rather than a bespoke picker.
//
// Mounted once in `App.vue`, next to `ConfirmationDialog`: choosing a mode
// here and pressing "Continue" calls `useResetStore().requestReset()`,
// which itself calls `useOperationStore().request(...)` — the *second*,
// shared confirmation step then renders through `ConfirmationDialog.vue`
// exactly like every other mutation in this workspace, never a bespoke
// confirm button here.

import { nextTick, ref, watch } from "vue";

import type { ResetModeDto } from "../services/dto";
import { useResetStore } from "../stores/reset";
import { focusFirst, handleFocusTrapKeydown } from "./focusTrap";

const reset = useResetStore();
const dialogEl = ref<HTMLElement | null>(null);

// Same focus-on-open / Tab-trap / Escape-to-cancel wiring as
// `ConfirmationDialog.vue` (US-055 criterion 1 / WCAG 2.1.2) — this dialog
// predates that one's focus handling and needs it just as much, since it
// is a real modal (`role="dialog"`, `aria-modal="true"`) with its own
// radio-button choice, not merely a confirm/cancel pair.
watch(
  () => reset.isOpen,
  (isOpen) => {
    if (isOpen) {
      void nextTick(() => focusFirst(dialogEl.value));
    }
  },
);

function onDialogKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    reset.close();
    return;
  }
  if (dialogEl.value) {
    handleFocusTrapKeydown(dialogEl.value, event);
  }
}

const MODES: { mode: ResetModeDto; label: string; description: string }[] = [
  {
    mode: "soft",
    label: "Soft",
    description: "HEAD moves only; index and working tree preserved (changes become staged).",
  },
  {
    mode: "mixed",
    label: "Mixed",
    description: "HEAD and index move; working tree preserved (changes become unstaged).",
  },
  {
    mode: "hard",
    label: "Hard",
    description: "HEAD, index and working tree all move. Uncommitted changes are discarded.",
  },
];

function requestReset(): void {
  void reset.requestReset();
}
</script>

<template>
  <div v-if="reset.isOpen" class="history-editing-overlay">
    <div
      ref="dialogEl"
      class="history-editing-dialog"
      role="dialog"
      aria-modal="true"
      @keydown="onDialogKeydown"
    >
      <h3>Reset to {{ reset.target?.shortHash }}</h3>

      <fieldset class="history-editing-modes">
        <label v-for="entry in MODES" :key="entry.mode" class="history-editing-mode">
          <input
            type="radio"
            name="reset-mode"
            :value="entry.mode"
            :checked="reset.mode === entry.mode"
            @change="reset.setMode(entry.mode)"
          />
          <span class="history-editing-mode__label">{{ entry.label }}</span>
          <span class="history-editing-mode__description">{{ entry.description }}</span>
          <span v-if="entry.mode === 'hard'" class="history-editing-mode__loss">
            {{ reset.predictedLossFileCount }} uncommitted change{{
              reset.predictedLossFileCount === 1 ? "" : "s"
            }}
            would be PERMANENTLY DISCARDED.
          </span>
        </label>
      </fieldset>

      <div class="history-editing-actions">
        <button @click="reset.close()">Cancel</button>
        <button class="confirm" @click="requestReset">Continue</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-editing-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 999;
}
.history-editing-dialog {
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-strong);
  border-radius: 4px;
  padding: 1rem 1.25rem;
  min-width: 22rem;
  max-width: 34rem;
}
.history-editing-modes {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  border: none;
  padding: 0;
  margin: 0.5rem 0;
}
.history-editing-mode {
  display: grid;
  grid-template-columns: auto 1fr;
  column-gap: 0.5rem;
  align-items: baseline;
}
.history-editing-mode__label {
  font-weight: 600;
}
.history-editing-mode__description {
  grid-column: 2;
  opacity: 0.85;
  font-size: 0.9rem;
}
.history-editing-mode__loss {
  grid-column: 2;
  color: var(--color-danger);
  font-weight: 600;
  font-size: 0.9rem;
}
.history-editing-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  margin-top: 0.75rem;
}
.confirm {
  font-weight: 600;
}
</style>
