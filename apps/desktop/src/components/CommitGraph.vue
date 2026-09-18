<script setup lang="ts">
// Interactive commit graph (US-067). Renders the Core-computed layout
// (`useCommitGraphStore`) as an SVG connector overlay plus one row per
// commit, virtualized so only the rows currently in view are ever mounted
// (criterion 2) — a large repository's history never renders thousands of
// DOM/SVG nodes at once just because it has been paginated in.

import { computed, onMounted, ref } from "vue";

import { useCommitGraphStore } from "../stores/graph";
import { useMergeStore } from "../stores/merge";
import { useResetStore } from "../stores/reset";
import {
  DEFAULT_LANE_WIDTH,
  DEFAULT_ROW_HEIGHT,
  nodeGlyph,
  rowConnectors,
  totalHeight,
  visibleRange,
} from "./commitGraphLayout";

const graph = useCommitGraphStore();
const merge = useMergeStore();
const resetStore = useResetStore();

const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);
const contextMenu = ref<{ x: number; y: number; hash: string } | null>(null);

const laneCountForWidth = computed(() => Math.max(graph.laneCount, 1));
const laneAreaWidth = computed(() => laneCountForWidth.value * DEFAULT_LANE_WIDTH);
const canvasHeight = computed(() => totalHeight(graph.rows.length));

const range = computed(() =>
  visibleRange(scrollTop.value, viewportHeight.value, graph.rows.length),
);

const visibleRows = computed(() =>
  graph.rows.slice(range.value.start, range.value.end).map((row, offset) => ({
    row,
    index: range.value.start + offset,
  })),
);

const visibleConnectors = computed(() =>
  rowConnectors(graph.rows.slice(range.value.start, range.value.end)).map((connector) => ({
    ...connector,
    y1: connector.y1 + range.value.start * DEFAULT_ROW_HEIGHT,
    y2: connector.y2 + range.value.start * DEFAULT_ROW_HEIGHT,
  })),
);

function glyphFor(row: (typeof graph.rows)[number], index: number) {
  return nodeGlyph(row, index);
}

function onScroll(): void {
  if (viewport.value) {
    scrollTop.value = viewport.value.scrollTop;
    viewportHeight.value = viewport.value.clientHeight;
  }
  maybeLoadMore();
}

function maybeLoadMore(): void {
  if (!viewport.value || !graph.hasMore || graph.isLoading) {
    return;
  }
  const { scrollTop: top, scrollHeight, clientHeight } = viewport.value;
  if (scrollHeight - (top + clientHeight) < DEFAULT_ROW_HEIGHT * 4) {
    void graph.loadMore();
  }
}

function onRowClick(hash: string): void {
  graph.select(hash);
  contextMenu.value = null;
}

function onRowHover(hash: string | null): void {
  graph.hover(hash);
}

function onRowContextMenu(event: MouseEvent, hash: string): void {
  event.preventDefault();
  graph.select(hash);
  contextMenu.value = { x: event.clientX, y: event.clientY, hash };
}

function closeContextMenu(): void {
  contextMenu.value = null;
}

function copySelectedHash(): void {
  const hash = contextMenu.value?.hash;
  if (hash) {
    void navigator.clipboard?.writeText(hash);
  }
  closeContextMenu();
}

/** The full commit the context menu is currently open for — looked up from
 * the already-loaded rows by hash, so the menu never needs a second read
 * (T-238/US-086; T-239/US-087; T-240/US-088). `undefined` while no menu is
 * open, or in the vanishingly unlikely case the row scrolled out of the
 * currently loaded page between opening the menu and clicking an action. */
const contextCommit = computed(() =>
  graph.rows.find((row) => row.commit.hash === contextMenu.value?.hash)?.commit,
);

/** Cherry-picks the right-clicked commit onto the current branch (T-238/
 * US-086 criterion 1). A merge commit always uses this workspace's fixed
 * first-parent policy (`isMerge`), named explicitly in the resulting
 * confirmation rather than left implicit. */
function requestCherryPick(): void {
  const commit = contextCommit.value;
  closeContextMenu();
  if (!commit) {
    return;
  }
  void merge.requestCherryPick(commit.hash, commit.shortHash, commit.isMerge);
}

/** Reverts the right-clicked commit (T-239/US-087 criterion 1), mirroring
 * `requestCherryPick`. */
function requestRevert(): void {
  const commit = contextCommit.value;
  closeContextMenu();
  if (!commit) {
    return;
  }
  void merge.requestRevert(commit.hash, commit.shortHash, commit.isMerge);
}

/** Opens the reset-mode chooser (`HistoryEditingPanel.vue`) for the
 * right-clicked commit (T-240/US-088 criterion 1) — never itself a
 * mutation. */
function requestReset(): void {
  const commit = contextCommit.value;
  closeContextMenu();
  if (!commit) {
    return;
  }
  resetStore.open({ hash: commit.hash, shortHash: commit.shortHash });
}

onMounted(() => {
  if (viewport.value) {
    viewportHeight.value = viewport.value.clientHeight;
  }
  if (graph.rows.length === 0) {
    void graph.loadFirstPage();
  }
});
</script>

<template>
  <div class="commit-graph" @click="closeContextMenu">
    <p v-if="graph.lastError" class="error">{{ graph.lastError.message }}</p>
    <div
      ref="viewport"
      class="commit-graph__viewport"
      @scroll="onScroll"
      @mouseleave="onRowHover(null)"
    >
      <div class="commit-graph__canvas" :style="{ height: `${canvasHeight}px` }">
        <svg
          class="commit-graph__connectors"
          :width="laneAreaWidth"
          :height="canvasHeight"
        >
          <line
            v-for="(connector, i) in visibleConnectors"
            :key="i"
            :x1="connector.x1"
            :y1="connector.y1"
            :x2="connector.x2"
            :y2="connector.y2"
            :class="{
              'commit-graph__edge--unresolved': !connector.resolved,
              'commit-graph__edge--diagonal': connector.kind === 'diagonal',
            }"
            class="commit-graph__edge"
          />
        </svg>

        <div
          v-for="{ row, index } in visibleRows"
          :key="row.commit.hash"
          class="commit-graph__row"
          :class="{
            'commit-graph__row--selected': graph.selectedHash === row.commit.hash,
            'commit-graph__row--hover': graph.hoverHash === row.commit.hash,
          }"
          :style="{ top: `${index * DEFAULT_ROW_HEIGHT}px`, height: `${DEFAULT_ROW_HEIGHT}px` }"
          @click.stop="onRowClick(row.commit.hash)"
          @mouseenter="onRowHover(row.commit.hash)"
          @contextmenu="onRowContextMenu($event, row.commit.hash)"
        >
          <span
            class="commit-graph__node"
            :class="`commit-graph__node--${glyphFor(row, index).kind}`"
            :style="{ left: `${glyphFor(row, index).cx}px` }"
          />
          <span class="commit-graph__label" :style="{ paddingLeft: `${laneAreaWidth + 8}px` }">
            <code>{{ row.commit.shortHash }}</code>
            <span v-if="row.edges.some((e) => !e.resolved)" class="commit-graph__continues">
              ⋯
            </span>
            {{ row.commit.subject }}
          </span>
        </div>

        <p v-if="graph.isLoading" class="commit-graph__loading">Loading…</p>
      </div>
    </div>

    <div
      v-if="contextMenu"
      class="commit-graph__context-menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      @click.stop
    >
      <button @click="copySelectedHash">Copy hash ({{ contextMenu.hash.slice(0, 8) }})</button>
      <button @click="requestCherryPick">Cherry-pick</button>
      <button @click="requestRevert">Revert</button>
      <button @click="requestReset">Reset to here…</button>
    </div>
  </div>
</template>

<style scoped>
.commit-graph {
  position: relative;
  height: 100%;
}
.commit-graph__viewport {
  height: 100%;
  overflow-y: auto;
  position: relative;
}
.commit-graph__canvas {
  position: relative;
}
.commit-graph__connectors {
  position: absolute;
  top: 0;
  left: 0;
  pointer-events: none;
}
.commit-graph__edge {
  stroke: #888;
  stroke-width: 2;
}
.commit-graph__edge--unresolved {
  stroke-dasharray: 3 3;
}
.commit-graph__row {
  position: absolute;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  cursor: pointer;
  white-space: nowrap;
}
.commit-graph__row--hover {
  background: rgba(255, 255, 255, 0.06);
}
.commit-graph__row--selected {
  background: rgba(100, 180, 255, 0.18);
}
.commit-graph__node {
  position: absolute;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #6cf;
  transform: translateX(-4px);
}
.commit-graph__node--root {
  border-radius: 0;
}
.commit-graph__node--merge {
  background: #f9c74f;
}
.commit-graph__label {
  overflow: hidden;
  text-overflow: ellipsis;
}
.commit-graph__continues {
  opacity: 0.7;
  margin: 0 0.25rem;
}
.commit-graph__loading {
  position: sticky;
  bottom: 0;
}
.commit-graph__context-menu {
  position: fixed;
  background: #2a2a2a;
  border: 1px solid #555;
  padding: 0.25rem;
  z-index: 10;
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
}
.commit-graph__context-menu button {
  text-align: left;
}
.error {
  color: #c0392b;
}
</style>
