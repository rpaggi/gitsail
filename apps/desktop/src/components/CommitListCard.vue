<script setup lang="ts">
// The "Commit Graph" card: the commit list plus the controls that scope
// it. Shared by the Overview and Commits views so the two cannot drift.
//
// Every control is backed by real state. The branch filter is the graph
// store's own `branchFilter` (changing it re-requests the first page for
// that branch). The "Show remote branches" switch is local view state that
// controls whether remote-tracking refs render as chips on a row — on a
// repository with several remotes, every commit otherwise carries a row of
// near-duplicate chips that pushes the local branch names out of view.
// It deliberately does not claim to change *which commits* are listed:
// that is the branch filter's job, and a switch that silently did both
// would make neither predictable.

import { computed, ref } from "vue";

import CommitGraph from "./CommitGraph.vue";
import GsCard from "./GsCard.vue";
import GsIcon from "./GsIcon.vue";
import { useBranchesStore } from "../stores/branches";
import { useCommitGraphStore } from "../stores/graph";

withDefaults(defineProps<{ title?: string }>(), { title: "Commit Graph" });

const graph = useCommitGraphStore();
const branches = useBranchesStore();

const showRemoteBranches = ref(true);

const branchOptions = computed(() =>
  branches.branches.filter((branch) => branch.kind.kind === "local").map((branch) => branch.name),
);

function onBranchFilterChange(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  void graph.loadFirstPage(value === "" ? null : value);
}

function focusSearch(): void {
  document.getElementById("gitsail-search-input")?.focus();
}

const menuOpen = ref(false);

function reload(): void {
  menuOpen.value = false;
  void graph.loadFirstPage(graph.branchFilter);
}

function copySelectedHash(): void {
  menuOpen.value = false;
  if (graph.selectedHash) {
    void navigator.clipboard?.writeText(graph.selectedHash);
  }
}
</script>

<template>
  <GsCard class="commit-list-card" :title="title" fill bleed>
    <template #actions>
      <label class="sr-only" for="gitsail-graph-branch-filter">Filter history by branch</label>
      <select
        id="gitsail-graph-branch-filter"
        class="commit-list-card__filter"
        :value="graph.branchFilter ?? ''"
        @change="onBranchFilterChange"
      >
        <option value="">All Branches</option>
        <option v-for="name in branchOptions" :key="name" :value="name">{{ name }}</option>
      </select>

      <button
        type="button"
        role="switch"
        class="commit-list-card__switch"
        :aria-checked="showRemoteBranches"
        @click="showRemoteBranches = !showRemoteBranches"
      >
        <span class="commit-list-card__switch-track" aria-hidden="true">
          <span class="commit-list-card__switch-thumb" />
        </span>
        <span class="commit-list-card__switch-label">Show Remote Branches</span>
      </button>

      <button
        type="button"
        class="btn-icon btn-ghost"
        aria-label="Search commits"
        title="Search commits"
        @click="focusSearch"
      >
        <GsIcon name="search" :size="15" />
      </button>

      <div class="commit-list-card__menu-wrap">
        <button
          type="button"
          class="btn-icon btn-ghost"
          aria-label="More commit list actions"
          aria-haspopup="menu"
          :aria-expanded="menuOpen"
          @click="menuOpen = !menuOpen"
        >
          <GsIcon name="kebab" :size="15" />
        </button>
        <div
          v-if="menuOpen"
          class="commit-list-card__menu"
          role="menu"
          aria-label="Commit list actions"
          @keydown.esc="menuOpen = false"
        >
          <button type="button" role="menuitem" class="btn-ghost" @click="reload">
            Reload history
          </button>
          <button
            type="button"
            role="menuitem"
            class="btn-ghost"
            :disabled="!graph.selectedHash"
            @click="copySelectedHash"
          >
            Copy selected hash
          </button>
        </div>
      </div>
    </template>

    <CommitGraph :show-remote-branches="showRemoteBranches" />
  </GsCard>
</template>

<style scoped>
.commit-list-card__filter {
  height: 28px;
  padding: 0 0.45rem;
  font-size: 0.8rem;
}

.commit-list-card__switch {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  height: 28px;
  padding: 0 0.3rem;
  background: transparent;
  border-color: transparent;
  color: var(--color-text-muted);
  font-size: 0.8rem;
}

.commit-list-card__switch:hover {
  background: transparent;
  border-color: transparent;
  color: var(--color-text);
}

.commit-list-card__switch-track {
  width: 34px;
  height: 18px;
  border-radius: var(--radius-pill);
  background: var(--color-border-strong);
  padding: 2px;
  transition: background 140ms ease;
  flex: none;
}

.commit-list-card__switch[aria-checked="true"] .commit-list-card__switch-track {
  background: var(--color-accent-solid);
}

.commit-list-card__switch-thumb {
  display: block;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #fff;
  transition: transform 140ms ease;
}

.commit-list-card__switch[aria-checked="true"] .commit-list-card__switch-thumb {
  transform: translateX(16px);
}

.commit-list-card__menu-wrap {
  position: relative;
}

.commit-list-card__menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  min-width: 12rem;
  padding: 0.3rem;
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  z-index: 50;
}

.commit-list-card__menu button {
  justify-content: flex-start;
  text-align: left;
  width: 100%;
}

/* The switch label and the branch filter are the first things to go when
   the card header runs out of room — both remain operable, the label just
   stops being spelled out next to the switch. */
@media (max-width: 78rem) {
  .commit-list-card__switch-label {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
  }
}
</style>
