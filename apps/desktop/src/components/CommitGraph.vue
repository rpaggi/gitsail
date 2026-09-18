<script setup lang="ts">
// Interactive commit graph (US-067). Renders the Core-computed layout
// (`useCommitGraphStore`) as an SVG connector overlay plus one row per
// commit, virtualized so only the rows currently in view are ever mounted
// (criterion 2) — a large repository's history never renders thousands of
// DOM/SVG nodes at once just because it has been paginated in.

import { computed, nextTick, onMounted, ref } from "vue";

import { useCommitGraphStore } from "../stores/graph";
import { useMergeStore } from "../stores/merge";
import { useResetStore } from "../stores/reset";
import { rovingNextIndex } from "./keyboardNav";
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
const contextMenuEl = ref<HTMLElement | null>(null);

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

/** DOM id for a commit row, shared between the row itself and the
 * viewport's `aria-activedescendant` (US-055 criterion 1) — the
 * WAI-ARIA "listbox with a virtualized/scrolling list" pattern: the
 * viewport, not each row, stays the one focusable/tabbable element (rows
 * come and go from the DOM as the list virtualizes), and
 * `aria-activedescendant` is how a screen reader is told which currently
 * rendered row counts as "focused" without literally moving DOM focus
 * onto it. */
function rowElementId(hash: string): string {
  return `commit-graph-row-${hash}`;
}

function openMenuAt(x: number, y: number, hash: string): void {
  graph.select(hash);
  contextMenu.value = { x, y, hash };
  // Move focus into the menu so keyboard users land somewhere actionable
  // immediately, whether the menu was opened by a right-click or by the
  // keyboard equivalent below — mirrors how `ConfirmationDialog.vue`/
  // `HistoryEditingPanel.vue` are expected to receive focus on open.
  void nextTick(() => {
    contextMenuEl.value?.querySelector<HTMLButtonElement>("button")?.focus();
  });
}

function onRowContextMenu(event: MouseEvent, hash: string): void {
  event.preventDefault();
  openMenuAt(event.clientX, event.clientY, hash);
}

/** Keyboard equivalent of right-clicking a row (US-055 criterion 1: every
 * mouse-only action needs a keyboard path) — the "Menu" key, or
 * Shift+F10, both of which are the standard OS/browser convention for
 * "open the context menu for whatever has focus". Positions the menu at
 * the focused row's own on-screen location so it never appears somewhere
 * unrelated to what it acts on. */
function onRequestContextMenuFromKeyboard(hash: string): void {
  const rowEl = document.getElementById(rowElementId(hash));
  const rect = rowEl?.getBoundingClientRect();
  if (rect) {
    openMenuAt(rect.left + 24, rect.bottom, hash);
  } else if (viewport.value) {
    const viewportRect = viewport.value.getBoundingClientRect();
    openMenuAt(viewportRect.left + 24, viewportRect.top + 24, hash);
  }
}

function closeContextMenu(): void {
  contextMenu.value = null;
}

/** Closes the menu and returns focus to the graph viewport (WCAG 2.1
 * "no keyboard trap" / focus-must-go-somewhere-sensible-on-close) — used
 * by Escape rather than the generic `closeContextMenu` alone, which a
 * mouse-driven close (clicking elsewhere) doesn't need. */
function closeContextMenuAndReturnFocus(): void {
  closeContextMenu();
  viewport.value?.focus();
}

/** The row index currently selected, or `-1` before anything has ever been
 * selected — arrow-key navigation below starts from row 0 in that case
 * (the first Down/Up press selects the first/last loaded row, same as a
 * native listbox with nothing pre-selected). */
const selectedRowIndex = computed(() =>
  graph.rows.findIndex((row) => row.commit.hash === graph.selectedHash),
);

/** Scrolls the viewport just enough to bring `index` into view — the
 * virtualized-list equivalent of `Element.scrollIntoView`, which can't be
 * used directly here since the row at `index` may not even be mounted yet
 * (it is what we are about to scroll to). Setting `scrollTop` fires the
 * viewport's own native `scroll` event, which `onScroll` already handles,
 * so the visible/rendered range updates the normal way. */
function scrollRowIntoView(index: number): void {
  const el = viewport.value;
  if (!el) {
    return;
  }
  const top = index * DEFAULT_ROW_HEIGHT;
  const bottom = top + DEFAULT_ROW_HEIGHT;
  if (top < el.scrollTop) {
    el.scrollTop = top;
  } else if (bottom > el.scrollTop + el.clientHeight) {
    el.scrollTop = bottom - el.clientHeight;
  }
}

/** Arrow-key/Home/End navigation of the commit list, plus the keyboard
 * context-menu shortcut (US-055 criterion 1: the graph's main actions —
 * moving the selection, and reaching cherry-pick/revert/reset — must be
 * reachable without a mouse). Non-wrapping: `Down` on the last *loaded*
 * row does nothing rather than jumping back to row 0, since more rows may
 * still load below (`graph.hasMore`). */
function onViewportKeydown(event: KeyboardEvent): void {
  if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
    event.preventDefault();
    const hash = graph.selectedHash ?? graph.rows[0]?.commit.hash;
    if (hash) {
      onRequestContextMenuFromKeyboard(hash);
    }
    return;
  }
  const current = selectedRowIndex.value >= 0 ? selectedRowIndex.value : 0;
  const next = rovingNextIndex(current, event.key, graph.rows.length, "vertical", false);
  if (next === null) {
    return;
  }
  event.preventDefault();
  graph.select(graph.rows[next].commit.hash);
  scrollRowIntoView(next);
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
      role="listbox"
      aria-label="Commit history"
      tabindex="0"
      :aria-activedescendant="graph.selectedHash ? rowElementId(graph.selectedHash) : undefined"
      @scroll="onScroll"
      @mouseleave="onRowHover(null)"
      @keydown="onViewportKeydown"
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
          :id="rowElementId(row.commit.hash)"
          :key="row.commit.hash"
          class="commit-graph__row"
          role="option"
          :aria-selected="graph.selectedHash === row.commit.hash"
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
      ref="contextMenuEl"
      class="commit-graph__context-menu"
      role="menu"
      :aria-label="`Actions for commit ${contextMenu.hash.slice(0, 8)}`"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      @click.stop
      @keydown.esc="closeContextMenuAndReturnFocus"
    >
      <button role="menuitem" @click="copySelectedHash">Copy hash ({{ contextMenu.hash.slice(0, 8) }})</button>
      <button role="menuitem" @click="requestCherryPick">Cherry-pick commit {{ contextMenu.hash.slice(0, 8) }}</button>
      <button role="menuitem" @click="requestRevert">Revert commit {{ contextMenu.hash.slice(0, 8) }}</button>
      <button role="menuitem" @click="requestReset">Reset to {{ contextMenu.hash.slice(0, 8) }}…</button>
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
