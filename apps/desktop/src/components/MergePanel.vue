<script setup lang="ts">
// Merge, conflict resolution, continue/abort (EPIC-16/T-231..T-233), and
// rebase/skip (EPIC-17/T-235). Mirrors `gitsail-tui`'s own wiring: a merge/
// rebase target is chosen through the same branch list `BranchPanel.vue`
// already lists (no bespoke picker), the outcome is always shown as its own
// distinct, persistent banner (never collapsed into the generic "Completed
// successfully" the shared `ConfirmationDialog.vue` shows for every
// operation — mirrors how `SyncPanel.vue`'s own `lastPullResult` banner
// already works for `pull`), and every mutation goes through
// `stores/operation.ts`'s shared confirm/run/result flow — this component
// never calls `services/merge.ts` directly.
//
// Rebase is folded into this same panel rather than a separate
// `RebasePanel.vue`: it reuses the identical conflicts-resolution section
// below (a rebase conflict is inspected/resolved exactly like a merge
// conflict) and the identical continue/abort block, now extended with
// skip — splitting it into its own component would only duplicate that
// markup and the `merge-panel__conflicts`/`merge-panel__actions` styling
// for no distinct concern (see `stores/merge.ts`'s own note on why the
// store itself stayed one store).

import { computed, onMounted, ref } from "vue";

import { useBranchesStore } from "../stores/branches";
import { useMergeStore } from "../stores/merge";
import type { ConflictedFileDto, ConflictSideContentDto } from "../services/dto";

const branches = useBranchesStore();
const merge = useMergeStore();

const mergeTarget = ref("");
const rebaseTarget = ref("");

/** The path currently expanded for inspection (T-232/US-080 criterion 2) —
 * distinct from `merge.inspectedPath`, which is only set once the sides
 * have actually loaded; this tracks the click itself so the panel can show
 * a "Loading…" state in between. */
const expandedPath = ref<string | null>(null);

const conflictedFiles = computed<ConflictedFileDto[]>(() => merge.conflictedFiles);

function stageLabel(stage: ConflictedFileDto["stage"]): string {
  switch (stage) {
    case "bothModified":
      return "both modified";
    case "bothAdded":
      return "both added";
    case "bothDeleted":
      return "both deleted";
    case "addedByUs":
      return "added by us";
    case "addedByThem":
      return "added by them";
    case "deletedByUs":
      return "deleted by us";
    case "deletedByThem":
      return "deleted by them";
    default:
      return stage;
  }
}

function sideLabel(content: ConflictSideContentDto): string {
  if (content.kind === "text") {
    const lines = content.text.split("\n");
    return `${lines[0] ?? ""} (${lines.length} line(s))`;
  }
  if (content.kind === "binary") {
    return "<binary content>";
  }
  return "<absent>";
}

function requestMerge(): void {
  const target = mergeTarget.value.trim();
  if (target.length === 0) {
    return;
  }
  void merge.requestMerge(target);
}

function requestRebase(): void {
  const target = rebaseTarget.value.trim();
  if (target.length === 0) {
    return;
  }
  void merge.requestRebase(target);
}

async function toggleInspect(path: string): Promise<void> {
  if (expandedPath.value === path) {
    expandedPath.value = null;
    return;
  }
  expandedPath.value = path;
  await merge.inspectConflict(path);
}

onMounted(() => {
  void branches.load();
  void merge.refreshInProgressOperation();
});
</script>

<template>
  <div class="merge-panel">
    <h3>Merge</h3>

    <div class="merge-panel__request">
      <select v-model="mergeTarget">
        <option value="" disabled>Choose a reference…</option>
        <option v-for="branch in branches.branches" :key="branch.name" :value="branch.name">
          {{ branch.name }}{{ branch.isCurrent ? " (current)" : "" }}
        </option>
      </select>
      <button :disabled="mergeTarget.trim().length === 0" @click="requestMerge">Merge into current</button>
    </div>

    <p v-if="merge.lastMergeResult" class="merge-panel__result" :class="{ conflict: merge.lastMergeResult.outcome === 'conflict' }">
      <template v-if="merge.lastMergeResult.outcome === 'fastForwarded'">
        Fast-forwarded to {{ merge.lastMergeResult.newHead.slice(0, 8) }}.
      </template>
      <template v-else-if="merge.lastMergeResult.outcome === 'mergeCommitCreated'">
        Merge commit {{ merge.lastMergeResult.hash.slice(0, 8) }} created.
      </template>
      <template v-else>
        CONFLICT — {{ merge.lastMergeResult.conflictedFiles.length }} file(s) need resolution below.
      </template>
    </p>

    <h3>Rebase</h3>

    <div class="merge-panel__request">
      <select v-model="rebaseTarget">
        <option value="" disabled>Choose a base…</option>
        <option v-for="branch in branches.branches" :key="branch.name" :value="branch.name">
          {{ branch.name }}{{ branch.isCurrent ? " (current)" : "" }}
        </option>
      </select>
      <button :disabled="rebaseTarget.trim().length === 0" @click="requestRebase">Rebase current onto…</button>
    </div>

    <p v-if="merge.lastRebaseResult" class="merge-panel__result" :class="{ conflict: merge.lastRebaseResult.outcome === 'conflict' }">
      <template v-if="merge.lastRebaseResult.outcome === 'completed'">
        Rebased onto {{ merge.lastRebaseResult.newHead.slice(0, 8) }}.
      </template>
      <template v-else>
        CONFLICT — {{ merge.lastRebaseResult.conflictedFiles.length }} file(s) need resolution below.
      </template>
    </p>

    <section v-if="merge.hasConflicts" class="merge-panel__conflicts">
      <h4>Conflicted files ({{ conflictedFiles.length }})</h4>
      <ul>
        <li v-for="file in conflictedFiles" :key="file.path" class="merge-panel__conflict">
          <div class="merge-panel__conflict-header">
            <button class="link" @click="toggleInspect(file.path)">{{ file.path }}</button>
            <span class="stage">{{ stageLabel(file.stage) }}</span>
          </div>

          <div v-if="expandedPath === file.path" class="merge-panel__sides">
            <p v-if="merge.conflictError" class="error">{{ merge.conflictError.message }}</p>
            <template v-else-if="merge.inspectedSides && merge.inspectedPath === file.path">
              <p>base: {{ sideLabel(merge.inspectedSides.base) }}</p>
              <p>ours: {{ sideLabel(merge.inspectedSides.ours) }}</p>
              <p>theirs: {{ sideLabel(merge.inspectedSides.theirs) }}</p>
            </template>
            <p v-else>Loading…</p>
          </div>

          <div class="merge-panel__conflict-actions">
            <button @click="merge.markResolved(file.path)">Mark resolved</button>
            <button @click="merge.takeSide(file.path, 'ours')">Take ours</button>
            <button @click="merge.takeSide(file.path, 'theirs')">Take theirs</button>
          </div>
        </li>
      </ul>
    </section>

    <div v-if="merge.inProgressOperation.kind !== 'none'" class="merge-panel__actions">
      <button :disabled="!merge.supportsContinue" @click="merge.requestContinue()">Continue</button>
      <button :disabled="!merge.supportsSkip" @click="merge.requestSkip()">Skip</button>
      <button :disabled="!merge.supportsAbort" @click="merge.requestAbort()">Abort</button>
    </div>
  </div>
</template>

<style scoped>
.merge-panel {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}
.merge-panel__request {
  display: flex;
  gap: 0.5rem;
}
.merge-panel__result {
  margin: 0;
  opacity: 0.85;
}
.merge-panel__result.conflict {
  color: #e0a030;
  font-weight: 600;
  opacity: 1;
}
.merge-panel__conflicts ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.merge-panel__conflict {
  border-top: 1px solid #444;
  padding: 0.35rem 0;
}
.merge-panel__conflict-header {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
}
.stage {
  opacity: 0.7;
  font-size: 0.85rem;
}
.merge-panel__sides {
  font-size: 0.85rem;
  opacity: 0.85;
  margin: 0.25rem 0;
}
.merge-panel__conflict-actions {
  display: flex;
  gap: 0.4rem;
  margin-top: 0.25rem;
}
.merge-panel__actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.4rem;
}
.link {
  background: none;
  border: none;
  color: inherit;
  text-decoration: underline;
  cursor: pointer;
  padding: 0;
  font: inherit;
}
.error {
  color: #c0392b;
}
</style>
