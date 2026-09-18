<script setup lang="ts">
// Stage/unstage and compose a commit (US-058), with drag-and-drop between
// the two lists as the one drag/drop surface T-196/US-063 introduces
// (`dragDropStaging.ts` is the single source of truth for what a drop
// means; every drop action here also has the adjacent button as its
// keyboard/click alternative, per US-063 criterion 3).

import { useStagingStore } from "../stores/staging";
import { useOperationStore } from "../stores/operation";
import { useDiffStore } from "../stores/diff";
import type { FileChangeDto } from "../services/dto";
import {
  STAGING_DRAG_MIME_TYPE,
  dropAction,
  parseDragPayload,
  serializeDragPayload,
  type StagingZone,
} from "./dragDropStaging";

const staging = useStagingStore();
const operation = useOperationStore();
const diff = useDiffStore();

function viewDiff(file: FileChangeDto, staged: boolean): void {
  void diff.load(file.path, staged);
}

function onDragStart(event: DragEvent, file: FileChangeDto, zone: StagingZone): void {
  event.dataTransfer?.setData(
    STAGING_DRAG_MIME_TYPE,
    serializeDragPayload({ path: file.path, sourceZone: zone }),
  );
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
  }
}

function onDragOver(event: DragEvent): void {
  // Accepting the drop (so the browser shows a "move" cursor) is safe to
  // do unconditionally here: an invalid/foreign payload is still rejected
  // in `onDrop` by `parseDragPayload`/`dropAction` before anything mutates.
  if (event.dataTransfer?.types.includes(STAGING_DRAG_MIME_TYPE)) {
    event.preventDefault();
  }
}

function onDrop(event: DragEvent, zone: StagingZone): void {
  event.preventDefault();
  const raw = event.dataTransfer?.getData(STAGING_DRAG_MIME_TYPE);
  if (!raw) {
    return; // not a GitSail staging drag (e.g. a foreign OS drag) — no-op
  }
  const payload = parseDragPayload(raw);
  if (!payload) {
    return;
  }
  const action = dropAction(payload, zone);
  if (action === "stage") {
    void staging.stageFiles([payload.path]);
  } else if (action === "unstage") {
    void staging.unstageFiles([payload.path]);
  }
  // action === null (dropped back onto its own zone): intentionally a
  // no-op, never guessed at as a toggle.
}
</script>

<template>
  <div class="staging-panel">
    <div class="staging-panel__columns">
      <section
        class="staging-panel__column"
        @dragover="onDragOver"
        @drop="onDrop($event, 'unstaged')"
      >
        <h3>Unstaged changes ({{ staging.unstagedFiles.length }})</h3>
        <ul>
          <li
            v-for="file in staging.unstagedFiles"
            :key="file.path"
            draggable="true"
            @dragstart="onDragStart($event, file, 'unstaged')"
          >
            <span class="staging-panel__path" @click="viewDiff(file, false)">{{ file.path }}</span>
            <button @click="staging.stageFiles([file.path])">Stage</button>
          </li>
        </ul>
      </section>

      <section
        class="staging-panel__column"
        @dragover="onDragOver"
        @drop="onDrop($event, 'staged')"
      >
        <h3>Staged changes ({{ staging.stagedFiles.length }})</h3>
        <ul>
          <li
            v-for="file in staging.stagedFiles"
            :key="file.path"
            draggable="true"
            @dragstart="onDragStart($event, file, 'staged')"
          >
            <span class="staging-panel__path" @click="viewDiff(file, true)">{{ file.path }}</span>
            <button @click="staging.unstageFiles([file.path])">Unstage</button>
          </li>
        </ul>
      </section>
    </div>

    <div class="staging-panel__composer">
      <textarea
        v-model="staging.message"
        placeholder="Commit message"
        rows="3"
        :disabled="operation.isBusy"
      />
      <button
        :disabled="staging.stagedFiles.length === 0 || staging.message.trim().length === 0 || operation.isBusy"
        @click="staging.requestCommit()"
      >
        Commit {{ staging.stagedFiles.length }} file{{ staging.stagedFiles.length === 1 ? "" : "s" }}
      </button>
      <p v-if="staging.lastCommitHash" class="staging-panel__result">
        Committed {{ staging.lastCommitHash.slice(0, 8) }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.staging-panel__columns {
  display: flex;
  gap: 1rem;
}
.staging-panel__column {
  flex: 1;
  min-height: 6rem;
  border: 1px dashed #444;
  padding: 0.5rem;
}
.staging-panel__column ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.staging-panel__column li {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  cursor: grab;
  padding: 0.15rem 0;
}
.staging-panel__path {
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
}
.staging-panel__path:hover {
  text-decoration: underline;
}
.staging-panel__composer {
  margin-top: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.staging-panel__composer textarea {
  width: 100%;
  box-sizing: border-box;
  font-family: inherit;
}
.staging-panel__result {
  margin: 0;
  opacity: 0.8;
}
</style>
