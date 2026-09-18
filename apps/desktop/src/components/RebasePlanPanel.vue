<script setup lang="ts">
// The interactive rebase plan overlay (T-236/US-084): lists the candidate
// commit range `useMergeStore().requestRebasePlan` reads, lets a person
// reorder entries with up/down buttons (drag-and-drop is explicitly out of
// scope for this story — up/down buttons are simpler and sufficient, per
// the task's own scope note) and reassign each entry's action, and shows a
// message field once an entry's action is `reword`. Confirming goes through
// the shared `useOperationStore` confirm/run/result flow via
// `merge.requestExecuteRebasePlan`, exactly like every other mutation in
// this app — this component never calls `services/merge.ts` directly and
// never talks to `execute_rebase_plan` on its own.
//
// A separate component (rather than folded further into `MergePanel.vue`)
// because this is materially more UI than the rest of that panel: a
// reorderable list with a per-entry action selector and a conditional
// message field, not a single request/result pair.

import { computed } from "vue";

import { useMergeStore } from "../stores/merge";
import { useOperationStore } from "../stores/operation";
import type { RebaseActionDto } from "../services/dto";

const merge = useMergeStore();
const operation = useOperationStore();

const actions: RebaseActionDto[] = ["pick", "reword", "squash", "fixup", "drop"];

const entries = computed(() => merge.rebasePlan?.entries ?? []);

/** Whether the confirm/cancel controls should be disabled — while an
 * operation this plan started is itself confirming/running, the generic
 * `ConfirmationDialog.vue` is what the person interacts with instead. */
const isBusy = computed(() => operation.status !== "idle");

function onActionChange(index: number, event: Event): void {
  const value = (event.target as HTMLSelectElement).value as RebaseActionDto;
  merge.setRebasePlanAction(index, value);
}

function onMessageInput(index: number, event: Event): void {
  merge.setRebasePlanMessage(index, (event.target as HTMLTextAreaElement).value);
}

function confirm(): void {
  void merge.requestExecuteRebasePlan();
}
</script>

<template>
  <section v-if="merge.isLoadingRebasePlan || merge.rebasePlan || merge.rebasePlanError" class="rebase-plan">
    <h3>Interactive Rebase Plan</h3>

    <p v-if="merge.isLoadingRebasePlan">Loading the candidate commit range…</p>

    <template v-else-if="merge.rebasePlan">
      <p class="rebase-plan__summary">
        {{ entries.length }} candidate commit(s) onto '{{ merge.rebasePlan.ontoRevision }}'
      </p>

      <p v-if="entries.length === 0">Nothing to reapply — already up to date.</p>

      <ol v-else class="rebase-plan__entries">
        <li v-for="(entry, index) in entries" :key="entry.commit" class="rebase-plan__entry">
          <div class="rebase-plan__entry-row">
            <span class="rebase-plan__move">
              <button
                :disabled="isBusy || index === 0"
                title="Move up"
                :aria-label="`Move ${entry.shortHash} up`"
                @click="merge.moveRebasePlanEntry(index, 'up')"
              >
                ↑
              </button>
              <button
                :disabled="isBusy || index === entries.length - 1"
                title="Move down"
                :aria-label="`Move ${entry.shortHash} down`"
                @click="merge.moveRebasePlanEntry(index, 'down')"
              >
                ↓
              </button>
            </span>
            <label :for="`rebase-plan-action-${entry.commit}`" class="sr-only">Action for {{ entry.shortHash }}</label>
            <select
              :id="`rebase-plan-action-${entry.commit}`"
              :disabled="isBusy"
              :value="entry.action"
              @change="onActionChange(index, $event)"
            >
              <option v-for="action in actions" :key="action" :value="action">{{ action }}</option>
            </select>
            <span class="rebase-plan__hash">{{ entry.shortHash }}</span>
            <span class="rebase-plan__subject">{{ entry.subject }}</span>
          </div>

          <textarea
            v-if="entry.action === 'reword'"
            class="rebase-plan__message"
            :disabled="isBusy"
            :value="entry.messageOverride ?? entry.subject"
            placeholder="New commit message…"
            :aria-label="`New commit message for ${entry.shortHash}`"
            @input="onMessageInput(index, $event)"
          ></textarea>
        </li>
      </ol>
    </template>

    <p v-if="merge.rebasePlanError" class="rebase-plan__error">{{ merge.rebasePlanError.message }}</p>

    <div class="rebase-plan__actions">
      <button :disabled="isBusy || !merge.rebasePlan" @click="confirm">Confirm plan</button>
      <button :disabled="isBusy" @click="merge.closeRebasePlan()">Cancel</button>
    </div>
  </section>
</template>

<style scoped>
.rebase-plan {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  border-top: 1px solid #444;
  padding-top: 0.5rem;
}
.rebase-plan__summary {
  margin: 0;
  opacity: 0.85;
}
.rebase-plan__entries {
  list-style: none;
  margin: 0;
  padding: 0;
}
.rebase-plan__entry {
  border-top: 1px solid #333;
  padding: 0.35rem 0;
}
.rebase-plan__entry-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.rebase-plan__move {
  display: flex;
  gap: 0.15rem;
}
.rebase-plan__hash {
  opacity: 0.7;
  font-family: monospace;
}
.rebase-plan__subject {
  flex: 1;
}
.rebase-plan__message {
  margin-top: 0.3rem;
  width: 100%;
  min-height: 2.5rem;
}
.rebase-plan__error {
  color: #c0392b;
  margin: 0;
}
.rebase-plan__actions {
  display: flex;
  gap: 0.5rem;
}
</style>
