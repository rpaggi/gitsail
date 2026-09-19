<script setup lang="ts">
// "Recent Activity": the newest commits, as a dotted timeline.
//
// The source is the commit graph's own first page — the same rows the
// commit list renders — rather than a separate feed, because a separate
// feed is exactly what GitSail does not have. The heading says "Recent
// Activity" and the content is recent commits; nothing here implies a
// broader event stream (pushes, PR comments) that the app cannot see.

import { computed, ref } from "vue";

import GsCard from "./GsCard.vue";
import GsIcon from "./GsIcon.vue";
import { formatRelativeTime } from "./relativeTime";
import { formatGitTimestamp } from "./timestampFormat";
import { useCommitGraphStore } from "../stores/graph";

const emit = defineEmits<{ (event: "view-all"): void }>();

const graph = useCommitGraphStore();

const nowSeconds = ref(Math.floor(Date.now() / 1000));

/** How many entries the timeline shows. The card is a glance, not a
 * history browser — "View all" goes to the list that is. */
const MAX_ENTRIES = 5;

const entries = computed(() =>
  graph.rows.slice(0, MAX_ENTRIES).map((row, index) => ({
    hash: row.commit.hash,
    subject: row.commit.subject,
    authorDate: row.commit.authorDate,
    // Cycled through the lane palette so the timeline reads as part of the
    // same system as the graph beside it, not a second color language.
    colorIndex: index % 6,
  })),
);
</script>

<template>
  <GsCard title="Recent Activity" as="h3">
    <template #actions>
      <button type="button" class="btn-ghost recent-activity__view-all" @click="emit('view-all')">
        View all
        <GsIcon name="arrow-right" :size="13" />
      </button>
    </template>

    <p v-if="graph.isLoading && entries.length === 0" class="recent-activity__empty" role="status">
      Loading recent commits&hellip;
    </p>
    <p v-else-if="entries.length === 0" class="recent-activity__empty">No commits yet.</p>

    <ol v-else class="recent-activity__list">
      <li
        v-for="entry in entries"
        :key="entry.hash"
        class="recent-activity__item"
        :class="[
          `recent-activity__item--${entry.colorIndex}`,
          { 'recent-activity__item--selected': graph.selectedHash === entry.hash },
        ]"
      >
        <span
          class="recent-activity__dot"
          aria-hidden="true"
        />
        <button type="button" class="recent-activity__trigger" @click="graph.select(entry.hash)">
          <span class="recent-activity__subject">{{ entry.subject }}</span>
          <time
            class="recent-activity__time"
            :title="formatGitTimestamp(entry.authorDate)"
            >{{ formatRelativeTime(entry.authorDate, nowSeconds) }}</time
          >
        </button>
      </li>
    </ol>
  </GsCard>
</template>

<style scoped>
.recent-activity__view-all {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.8rem;
  color: var(--color-accent);
  padding: 0.2rem 0.4rem;
}

.recent-activity__empty {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 0.82rem;
}

.recent-activity__list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.recent-activity__item {
  display: flex;
  align-items: flex-start;
  gap: 0.65rem;
  padding: 0.25rem 0.3rem 0.25rem 0;
  border-radius: var(--radius-sm);
  position: relative;
}

/* Each entry draws the segment down to the next one, tinted to match its
   own dot — so the timeline reads as one colored thread rather than a
   grey rule with colored beads on it. Drawn per item (and suppressed on
   the last) so the thread stops at the final dot instead of trailing into
   empty space. */
.recent-activity__item::before {
  content: "";
  position: absolute;
  left: 5px;
  top: 14px;
  bottom: -2px;
  width: 2px;
  background: currentColor;
  opacity: 0.55;
}

.recent-activity__item:last-child::before {
  display: none;
}

.recent-activity__item--0 {
  color: var(--color-lane-3);
}
.recent-activity__item--1 {
  color: var(--color-lane-1);
}
.recent-activity__item--2 {
  color: var(--color-lane-6);
}
.recent-activity__item--3 {
  color: var(--color-lane-2);
}
.recent-activity__item--4 {
  color: var(--color-lane-4);
}
.recent-activity__item--5 {
  color: var(--color-lane-5);
}

.recent-activity__item--selected {
  background: var(--color-accent-soft);
}

/* Inherits the item's lane color, so the dot and the segment below it
   can never disagree. */
.recent-activity__dot {
  position: relative;
  z-index: 1;
  flex: none;
  width: 12px;
  height: 12px;
  margin-top: 4px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 0 3px var(--color-surface);
}

.recent-activity__trigger {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 1px;
  padding: 0;
  background: transparent;
  border: none;
  text-align: left;
  cursor: pointer;
}

.recent-activity__trigger:hover {
  background: transparent;
  border-color: transparent;
}

.recent-activity__subject {
  font-size: 0.82rem;
  font-weight: 600;
  color: var(--color-text);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.recent-activity__trigger:hover .recent-activity__subject {
  color: var(--color-accent);
}

.recent-activity__time {
  font-size: 0.72rem;
  color: var(--color-text-muted);
}
</style>
