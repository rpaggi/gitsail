<script setup lang="ts">
// Interactive commit graph (US-067). Renders the Core-computed layout
// (`useCommitGraphStore`) as an SVG connector overlay plus one row per
// commit, virtualized so only the rows currently in view are ever mounted
// (criterion 2) — a large repository's history never renders thousands of
// DOM/SVG nodes at once just because it has been paginated in.
//
// Row geometry is passed explicitly into `commitGraphLayout.ts` rather
// than left on that module's defaults: this view renders two lines per
// commit (subject plus body) and needs a taller row than the module's
// 28px default, and every function there already takes `rowHeight`/
// `laneWidth` as parameters precisely so a caller can decide.

import { computed, nextTick, onMounted, ref } from "vue";

import GsAvatar from "./GsAvatar.vue";
import { useCommitGraphStore } from "../stores/graph";
import { useMergeStore } from "../stores/merge";
import { useResetStore } from "../stores/reset";
import { rovingNextIndex } from "./keyboardNav";
import { formatRelativeTime } from "./relativeTime";
import { formatGitTimestamp } from "./timestampFormat";
import { refBadges } from "./refBadges";
import {
  nodeGlyph,
  rowConnectors,
  totalHeight,
  visibleRange,
} from "./commitGraphLayout";

const props = withDefaults(
  defineProps<{
    /** Whether remote-tracking refs appear as chips on a row. Driven by
     * the containing card's "Show remote branches" toggle. */
    showRemoteBranches?: boolean;
  }>(),
  { showRemoteBranches: true },
);

const ROW_HEIGHT = 52;
/** Horizontal distance between lane centers, matching the mockup's own
 * graph gutter. Wide enough that a branch/merge connector reads as a
 * visible sideways move rather than a near-vertical kink. */
const LANE_WIDTH = 22;
/** Left inset of the lane gutter. Shared by the SVG overlay and the node
 * glyphs so the dots can never drift off their own lines — the two are
 * positioned by different mechanisms (an absolutely placed `<svg>` vs.
 * per-row absolute spans) and previously each carried their own copy. */
const GUTTER_LEFT = 14;
/** How many `--color-lane-*` tokens exist; lanes cycle through them. */
const LANE_COLORS = 6;

const graph = useCommitGraphStore();
const merge = useMergeStore();
const resetStore = useResetStore();

const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);
const contextMenu = ref<{ x: number; y: number; hash: string } | null>(null);
const contextMenuEl = ref<HTMLElement | null>(null);

/** Captured once per render pass rather than read per row: a relative
 * label recomputed from a fresh `Date.now()` inside a `v-for` would make
 * every row a new reactive dependency of the clock. */
const nowSeconds = ref(Math.floor(Date.now() / 1000));

const laneCountForWidth = computed(() => Math.max(graph.laneCount, 1));
const laneAreaWidth = computed(() => laneCountForWidth.value * LANE_WIDTH);
const canvasHeight = computed(() => totalHeight(graph.rows.length, ROW_HEIGHT));

const range = computed(() =>
  visibleRange(scrollTop.value, viewportHeight.value, graph.rows.length, ROW_HEIGHT),
);

const visibleRows = computed(() =>
  graph.rows.slice(range.value.start, range.value.end).map((row, offset) => ({
    row,
    index: range.value.start + offset,
    badges: refBadges(row.commit.decorations, { includeRemote: props.showRemoteBranches }),
  })),
);

const visibleConnectors = computed(() =>
  rowConnectors(
    graph.rows.slice(range.value.start, range.value.end),
    ROW_HEIGHT,
    LANE_WIDTH,
  ).map((connector) => ({
    ...connector,
    y1: connector.y1 + range.value.start * ROW_HEIGHT,
    y2: connector.y2 + range.value.start * ROW_HEIGHT,
  })),
);

/** The lane a connector lands in, recovered from its own geometry — the
 * layout module returns pixel coordinates, not lane indexes, and
 * inverting `laneX` here keeps that module free of color concerns. The
 * destination lane is what a diagonal should be colored by: that is the
 * branch it is drawing *into*. */
function laneColorIndex(x: number): number {
  return Math.round((x - LANE_WIDTH / 2) / LANE_WIDTH) % LANE_COLORS;
}

/**
 * The SVG path for one connector.
 *
 * A lane change is drawn as a cubic curve rather than a straight diagonal:
 * with control points pulled vertically, the line leaves its source lane
 * travelling straight down and arrives at its target the same way, which
 * is how a branch spawning or a merge converging actually reads. A
 * straight diagonal across a 52px row instead looks like a kinked
 * vertical, which is exactly the "it doesn't look like a graph" problem.
 *
 * Same-lane connectors stay a plain line — curving a straight run would
 * add wobble with no information in it.
 */
function connectorPath(connector: { kind: string; x1: number; y1: number; x2: number; y2: number }): string {
  const { x1, y1, x2, y2 } = connector;
  if (connector.kind !== "diagonal") {
    return `M ${x1} ${y1} L ${x2} ${y2}`;
  }
  const bend = (y2 - y1) * 0.5;
  return `M ${x1} ${y1} C ${x1} ${y1 + bend}, ${x2} ${y2 - bend}, ${x2} ${y2}`;
}

function glyphFor(row: (typeof graph.rows)[number], index: number) {
  return nodeGlyph(row, index, ROW_HEIGHT, LANE_WIDTH);
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
  if (scrollHeight - (top + clientHeight) < ROW_HEIGHT * 4) {
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
  const top = index * ROW_HEIGHT;
  const bottom = top + ROW_HEIGHT;
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
    <p v-if="graph.lastError" class="error commit-graph__error" role="alert">
      {{ graph.lastError.message }}
    </p>

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
      <p
        v-if="graph.rows.length === 0 && !graph.isLoading && !graph.lastError"
        class="commit-graph__empty"
      >
        No commits yet. Your first commit will show up here.
      </p>

      <div class="commit-graph__canvas" :style="{ height: `${canvasHeight}px` }">
        <svg
          class="commit-graph__connectors"
          :width="laneAreaWidth"
          :height="canvasHeight"
          :style="{ left: `${GUTTER_LEFT}px` }"
          aria-hidden="true"
        >
          <path
            v-for="(connector, i) in visibleConnectors"
            :key="i"
            :d="connectorPath(connector)"
            :class="[
              'commit-graph__edge',
              `commit-graph__edge--lane-${laneColorIndex(connector.x2)}`,
              { 'commit-graph__edge--unresolved': !connector.resolved },
            ]"
          />
        </svg>

        <div
          v-for="{ row, index, badges } in visibleRows"
          :id="rowElementId(row.commit.hash)"
          :key="row.commit.hash"
          class="commit-graph__row"
          role="option"
          :aria-selected="graph.selectedHash === row.commit.hash"
          :class="{
            'commit-graph__row--selected': graph.selectedHash === row.commit.hash,
            'commit-graph__row--hover': graph.hoverHash === row.commit.hash,
          }"
          :style="{ top: `${index * ROW_HEIGHT}px`, height: `${ROW_HEIGHT}px` }"
          @click.stop="onRowClick(row.commit.hash)"
          @mouseenter="onRowHover(row.commit.hash)"
          @contextmenu="onRowContextMenu($event, row.commit.hash)"
        >
          <span
            class="commit-graph__node"
            :class="[
              `commit-graph__node--${glyphFor(row, index).kind}`,
              `commit-graph__node--lane-${laneColorIndex(glyphFor(row, index).cx)}`,
            ]"
            :style="{ left: `${GUTTER_LEFT + glyphFor(row, index).cx}px` }"
          />

          <span
            class="commit-graph__content"
            :style="{ paddingLeft: `${GUTTER_LEFT + laneAreaWidth + 14}px` }"
          >
            <span class="commit-graph__headline">
              <span
                v-for="badge in badges"
                :key="`${badge.kind}-${badge.label}`"
                class="commit-graph__badge"
                :class="`commit-graph__badge--${badge.kind}`"
                >{{ badge.label }}</span
              >
              <span class="commit-graph__subject">{{ row.commit.subject }}</span>
              <span
                v-if="row.edges.some((e) => !e.resolved)"
                class="commit-graph__continues"
                title="History continues beyond the loaded page"
                >⋯</span
              >
            </span>
            <span class="commit-graph__body">{{ row.commit.body || row.commit.author.name }}</span>
          </span>

          <span class="commit-graph__meta">
            <span class="commit-graph__meta-text">
              <code class="commit-graph__hash">{{ row.commit.shortHash }}</code>
              <!-- The relative label is the scannable one; the exact date
                   stays available on hover rather than being dropped. -->
              <time
                class="commit-graph__time"
                :title="formatGitTimestamp(row.commit.authorDate)"
                >{{ formatRelativeTime(row.commit.authorDate, nowSeconds) }}</time
              >
            </span>
            <GsAvatar :author="row.commit.author" :size="28" />
          </span>
        </div>

        <p v-if="graph.isLoading" class="commit-graph__loading" role="status">Loading&hellip;</p>
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
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.commit-graph__error {
  margin: 0;
  padding: 0.6rem 1rem;
}

.commit-graph__viewport {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  position: relative;
}

.commit-graph__empty {
  padding: 2.5rem 1rem;
  text-align: center;
  color: var(--color-text-muted);
}

.commit-graph__canvas {
  position: relative;
}

.commit-graph__connectors {
  position: absolute;
  top: 0;
  /* `left` is set inline from GUTTER_LEFT so it cannot drift from the
     node glyphs, which are positioned by the same constant. */
  pointer-events: none;
  overflow: visible;
}

/* Heavy enough to read as a ribbon rather than a hairline — the mockup's
   own gutter is a ~4px stroke, and this is the element that makes the
   screen look like a commit graph at all. */
.commit-graph__edge {
  fill: none;
  stroke-width: 3.5;
  stroke-linecap: round;
}

/* Unresolved edges point at a commit that has not been paged in yet —
   dashed, so "the line stops here because we ran out of history" never
   reads as "the line stops here because the branch ended". */
.commit-graph__edge--unresolved {
  stroke-dasharray: 3 3;
  opacity: 0.65;
}

.commit-graph__edge--lane-0 {
  stroke: var(--color-lane-1);
}
.commit-graph__edge--lane-1 {
  stroke: var(--color-lane-2);
}
.commit-graph__edge--lane-2 {
  stroke: var(--color-lane-3);
}
.commit-graph__edge--lane-3 {
  stroke: var(--color-lane-4);
}
.commit-graph__edge--lane-4 {
  stroke: var(--color-lane-5);
}
.commit-graph__edge--lane-5 {
  stroke: var(--color-lane-6);
}

.commit-graph__row {
  position: absolute;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0 1rem 0 0;
  cursor: pointer;
  border-left: 2px solid transparent;
}

.commit-graph__row--hover {
  background: var(--color-surface-alt);
}

.commit-graph__row--selected,
.commit-graph__row--selected.commit-graph__row--hover {
  background: var(--color-accent-soft);
  /* A left edge as well as a tint: selection must not depend on a color
     difference alone (US-055 criterion 3). */
  border-left-color: var(--color-accent);
}

.commit-graph__node {
  position: absolute;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  /* `left` already includes GUTTER_LEFT (set inline), so the node only has
     to centre itself on its own lane. */
  transform: translateX(-50%);
  /* A ring in the row's own background colour punches the node out of the
     line running underneath it, so a dot never looks like a bulge. */
  box-shadow: 0 0 0 3px var(--color-surface);
  z-index: 1;
}

/* The selected commit's node grows and takes a bright halo — selection is
   legible in the gutter itself, not only from the row tint. */
.commit-graph__row--selected .commit-graph__node {
  width: 16px;
  height: 16px;
  box-shadow: 0 0 0 3px var(--color-surface), 0 0 0 5px var(--color-accent);
}

/* A root commit has no parents: square it off so the end of history is
   visually distinct from any other node, not just smaller. */
.commit-graph__node--root {
  border-radius: 2px;
}

/* A merge node is drawn hollow — two parents converge on it, and a ring
   reads as a junction rather than as one more commit on the lane. */
/* Grown past the solid nodes so the ring encloses the same amount of ink
   they do — a hollow glyph at the same box size reads as a smaller, fainter
   dot rather than as a deliberately different one. */
.commit-graph__node--merge {
  background: var(--color-surface) !important;
  width: 16px;
  height: 16px;
  border-width: 4px;
}

.commit-graph__node--lane-0 {
  background: var(--color-lane-1);
  border: 3px solid var(--color-lane-1);
}
.commit-graph__node--lane-1 {
  background: var(--color-lane-2);
  border: 3px solid var(--color-lane-2);
}
.commit-graph__node--lane-2 {
  background: var(--color-lane-3);
  border: 3px solid var(--color-lane-3);
}
.commit-graph__node--lane-3 {
  background: var(--color-lane-4);
  border: 3px solid var(--color-lane-4);
}
.commit-graph__node--lane-4 {
  background: var(--color-lane-5);
  border: 3px solid var(--color-lane-5);
}
.commit-graph__node--lane-5 {
  background: var(--color-lane-6);
  border: 3px solid var(--color-lane-6);
}

.commit-graph__content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 2px;
}

.commit-graph__headline {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  min-width: 0;
}

.commit-graph__subject {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-graph__body {
  font-size: 0.78rem;
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-graph__badge {
  flex: none;
  font-size: 0.69rem;
  font-weight: 600;
  line-height: 1.5;
  padding: 0 0.4rem;
  border-radius: var(--radius-sm);
  color: #fff;
  max-width: 11rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-graph__badge--branch {
  background: var(--color-badge-branch);
}
.commit-graph__badge--head {
  background: var(--color-badge-neutral);
}
.commit-graph__badge--tag {
  background: var(--color-lane-2);
  color: #1a1206;
}
.commit-graph__badge--remote {
  background: var(--color-badge-alt);
}

.commit-graph__continues {
  color: var(--color-text-faint);
  flex: none;
}

.commit-graph__meta {
  flex: none;
  display: flex;
  align-items: center;
  gap: 0.6rem;
}

.commit-graph__meta-text {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 1px;
}

.commit-graph__hash {
  font-size: 0.76rem;
  color: var(--color-text-muted);
}

.commit-graph__time {
  font-size: 0.72rem;
  color: var(--color-text-faint);
  white-space: nowrap;
}

.commit-graph__loading {
  position: sticky;
  bottom: 0;
  margin: 0;
  padding: 0.5rem 1rem;
  background: var(--color-surface);
  color: var(--color-text-muted);
  font-size: 0.8rem;
}

.commit-graph__context-menu {
  position: fixed;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  padding: 0.3rem;
  z-index: 100;
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
  min-width: 15rem;
}

.commit-graph__context-menu button {
  text-align: left;
  background: transparent;
  border-color: transparent;
}

/* Below this width the right-hand hash/time/avatar column and the two-line
   content cannot both hold their minimum size; the metadata is the part
   still reachable from the commit-details card, so it goes. */
@media (max-width: 48rem) {
  .commit-graph__meta-text {
    display: none;
  }
}
</style>
