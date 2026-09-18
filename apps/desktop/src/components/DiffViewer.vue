<script setup lang="ts">
// Diff viewer (US-057): toggles between unified and side-by-side without
// re-fetching or losing the current file/side (`stores/diff.ts`), shows an
// explicit banner instead of fabricated content for a binary or truncated
// file (criterion 2), and virtualizes its row list (criterion:
// "listas extensas não exigem render total") using the same
// scroll-driven windowing approach `CommitGraph.vue` already established
// for EPIC-13, applied here to flattened diff lines instead of graph rows.

import { computed, ref } from "vue";

import { useDiffStore } from "../stores/diff";
import { sideBySideRows, unifiedLines, type SideBySideRow, type UnifiedLine } from "./diffPresentation";
import { computeVisibleRange } from "./virtualList";
import type { FileDiffDto } from "../services/dto";

const ROW_HEIGHT = 20;

type FlatRow =
  | { kind: "fileHeader"; file: FileDiffDto }
  | { kind: "unified"; line: UnifiedLine }
  | { kind: "sideBySide"; row: SideBySideRow };

const diff = useDiffStore();

const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

const flatRows = computed<FlatRow[]>(() => {
  const files = diff.diff?.files ?? [];
  const rows: FlatRow[] = [];
  for (const file of files) {
    rows.push({ kind: "fileHeader", file });
    if (file.isBinary || file.truncated) {
      continue; // the header banner already says so; no fabricated lines
    }
    for (const hunk of file.hunks) {
      if (diff.mode === "unified") {
        for (const line of unifiedLines(hunk)) {
          rows.push({ kind: "unified", line });
        }
      } else {
        for (const row of sideBySideRows(hunk)) {
          rows.push({ kind: "sideBySide", row });
        }
      }
    }
  }
  return rows;
});

const range = computed(() =>
  computeVisibleRange({
    totalCount: flatRows.value.length,
    rowHeight: ROW_HEIGHT,
    viewportHeight: viewportHeight.value,
    scrollTop: scrollTop.value,
  }),
);

const visibleRows = computed(() => flatRows.value.slice(range.value.startIndex, range.value.endIndex));

function onScroll(): void {
  if (viewport.value) {
    scrollTop.value = viewport.value.scrollTop;
    viewportHeight.value = viewport.value.clientHeight;
  }
}

function originSymbol(origin: string): string {
  if (origin === "addition") return "+";
  if (origin === "deletion") return "-";
  return " ";
}
</script>

<template>
  <div class="diff-viewer">
    <div class="diff-viewer__toolbar">
      <button :class="{ active: diff.mode === 'unified' }" @click="diff.setMode('unified')">
        Unified
      </button>
      <button :class="{ active: diff.mode === 'sideBySide' }" @click="diff.setMode('sideBySide')">
        Side-by-side
      </button>
      <span v-if="diff.file" class="diff-viewer__file">{{ diff.file }}</span>
    </div>

    <p v-if="diff.lastError" class="error">{{ diff.lastError.message }}</p>
    <p v-else-if="diff.isLoading">Loading…</p>
    <p v-else-if="!diff.diff || diff.diff.files.length === 0">No content to show.</p>

    <div
      v-else
      ref="viewport"
      class="diff-viewer__viewport"
      @scroll="onScroll"
    >
      <div :style="{ height: `${range.offsetTop}px` }" />
      <template v-for="(row, i) in visibleRows" :key="i">
        <div v-if="row.kind === 'fileHeader'" class="diff-viewer__file-header">
          <strong>{{ row.file.path }}</strong>
          <span v-if="row.file.isBinary" class="diff-viewer__banner">
            Binary file — content not shown.
          </span>
          <span v-else-if="row.file.truncated" class="diff-viewer__banner">
            Diff truncated — content not fully available.
          </span>
        </div>

        <div
          v-else-if="row.kind === 'unified'"
          class="diff-viewer__line"
          :class="`diff-viewer__line--${row.line.origin}`"
        >
          <span class="diff-viewer__lineno">{{ row.line.oldLineNumber ?? "" }}</span>
          <span class="diff-viewer__lineno">{{ row.line.newLineNumber ?? "" }}</span>
          <span class="diff-viewer__origin">{{ originSymbol(row.line.origin) }}</span>
          <span class="diff-viewer__content">{{ row.line.content }}</span>
        </div>

        <div v-else class="diff-viewer__side-by-side-row">
          <span class="diff-viewer__cell" :class="{ 'diff-viewer__cell--changed': row.row.left?.changed }">
            <span class="diff-viewer__lineno">{{ row.row.left?.lineNumber ?? "" }}</span>
            <span class="diff-viewer__content">{{ row.row.left?.content ?? "" }}</span>
          </span>
          <span class="diff-viewer__cell" :class="{ 'diff-viewer__cell--changed': row.row.right?.changed }">
            <span class="diff-viewer__lineno">{{ row.row.right?.lineNumber ?? "" }}</span>
            <span class="diff-viewer__content">{{ row.row.right?.content ?? "" }}</span>
          </span>
        </div>
      </template>
      <div :style="{ height: `${range.offsetBottom}px` }" />
    </div>
  </div>
</template>

<style scoped>
.diff-viewer__toolbar {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  margin-bottom: 0.5rem;
}
.diff-viewer__toolbar button.active {
  font-weight: 600;
  text-decoration: underline;
}
.diff-viewer__file {
  opacity: 0.8;
}
.diff-viewer__viewport {
  height: 20rem;
  overflow-y: auto;
  font-family: monospace;
  font-size: 0.8rem;
}
.diff-viewer__file-header {
  padding: 0.25rem 0;
  border-top: 1px solid #444;
}
.diff-viewer__banner {
  margin-left: 0.5rem;
  opacity: 0.8;
  font-style: italic;
}
.diff-viewer__line,
.diff-viewer__side-by-side-row {
  display: flex;
  height: 20px;
  white-space: pre;
}
.diff-viewer__line--addition {
  background: rgba(46, 204, 113, 0.15);
}
.diff-viewer__line--deletion {
  background: rgba(231, 76, 60, 0.15);
}
.diff-viewer__lineno {
  width: 3rem;
  opacity: 0.6;
  flex-shrink: 0;
}
.diff-viewer__origin {
  width: 1rem;
  flex-shrink: 0;
}
.diff-viewer__content {
  flex: 1;
  overflow: hidden;
}
.diff-viewer__cell {
  flex: 1;
  display: flex;
  min-width: 0;
}
.diff-viewer__cell--changed {
  background: rgba(224, 160, 48, 0.15);
}
.error {
  color: #c0392b;
}
</style>
