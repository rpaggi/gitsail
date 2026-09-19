<script setup lang="ts">
// "Commit Details" for whatever is currently selected in the commit graph.
//
// Reads the already-loaded row rather than re-fetching the commit: the
// graph store holds the full `CommitDto` for every row it has paged in, so
// selecting a row in the list has nothing left to load. When the selection
// is a commit that is *not* in the loaded page (a deep link from the
// startup intent, say), the card says so instead of showing a blank shell.
//
// The mockup's footer shows a "N files changed +x -y" line. That figure
// requires a diff *of the commit*, and no store exposes one — the diff
// store is scoped to the working tree. Rather than fabricate it, the
// footer carries the facts the commit itself records: its parents, and
// whether it is a merge.

import { computed, ref } from "vue";

import GsAvatar from "./GsAvatar.vue";
import GsCard from "./GsCard.vue";
import GsIcon from "./GsIcon.vue";
import { formatRelativeTime } from "./relativeTime";
import { formatGitTimestamp } from "./timestampFormat";
import { refBadges } from "./refBadges";
import { useBlameStore } from "../stores/blame";
import { useCommitGraphStore } from "../stores/graph";
import { useMergeStore } from "../stores/merge";

const graph = useCommitGraphStore();
const merge = useMergeStore();
const blame = useBlameStore();

const nowSeconds = ref(Math.floor(Date.now() / 1000));
const menuOpen = ref(false);

const commit = computed(
  () => graph.rows.find((row) => row.commit.hash === graph.selectedHash)?.commit ?? null,
);

const badges = computed(() =>
  commit.value ? refBadges(commit.value.decorations, { includeRemote: true }) : [],
);

function copyHash(): void {
  menuOpen.value = false;
  if (commit.value) {
    void navigator.clipboard?.writeText(commit.value.hash);
  }
}

function cherryPick(): void {
  menuOpen.value = false;
  if (commit.value) {
    void merge.requestCherryPick(commit.value.hash, commit.value.shortHash, commit.value.isMerge);
  }
}

function revert(): void {
  menuOpen.value = false;
  if (commit.value) {
    void merge.requestRevert(commit.value.hash, commit.value.shortHash, commit.value.isMerge);
  }
}

/** Whether the blame overlay has a commit open — used only to avoid
 * offering an action that would do nothing. */
const blameBusy = computed(() => blame.isLoadingCommit);
</script>

<template>
  <GsCard title="Commit Details" as="h3">
    <template #actions>
      <div class="commit-details__menu-wrap">
        <button
          type="button"
          class="btn-icon btn-ghost"
          aria-label="Commit actions"
          aria-haspopup="menu"
          :aria-expanded="menuOpen"
          :disabled="!commit"
          @click="menuOpen = !menuOpen"
        >
          <GsIcon name="kebab" :size="15" />
        </button>
        <div
          v-if="menuOpen"
          class="commit-details__menu"
          role="menu"
          aria-label="Commit actions"
          @keydown.esc="menuOpen = false"
        >
          <button type="button" role="menuitem" class="btn-ghost" @click="copyHash">Copy full hash</button>
          <button type="button" role="menuitem" class="btn-ghost" :disabled="blameBusy" @click="cherryPick">
            Cherry-pick
          </button>
          <button type="button" role="menuitem" class="btn-ghost" :disabled="blameBusy" @click="revert">
            Revert
          </button>
        </div>
      </div>
    </template>

    <p v-if="!graph.selectedHash" class="commit-details__empty">
      Select a commit to see its details.
    </p>
    <p v-else-if="!commit" class="commit-details__empty" role="status">
      Commit {{ graph.selectedHash.slice(0, 8) }} is not in the loaded history yet — scroll the
      commit list to load it.
    </p>

    <template v-else>
      <div class="commit-details__head">
        <GsAvatar :author="commit.author" :size="30" />
        <p class="commit-details__subject">{{ commit.subject }}</p>
      </div>

      <p class="commit-details__meta">
        <code>{{ commit.shortHash }}</code>
        <span aria-hidden="true">&middot;</span>
        <time :title="formatGitTimestamp(commit.authorDate)">{{
          formatRelativeTime(commit.authorDate, nowSeconds)
        }}</time>
        <span aria-hidden="true">&middot;</span>
        <span>{{ commit.author.name }}</span>
      </p>

      <p v-if="commit.body" class="commit-details__body">{{ commit.body }}</p>

      <p v-if="badges.length > 0" class="commit-details__badges">
        <span
          v-for="badge in badges"
          :key="`${badge.kind}-${badge.label}`"
          class="commit-details__badge"
          :class="`commit-details__badge--${badge.kind}`"
          >{{ badge.label }}</span
        >
      </p>

      <p class="commit-details__footer">
        <span v-if="commit.isRoot">Root commit</span>
        <span v-else-if="commit.isMerge">Merge commit &middot; {{ commit.parents.length }} parents</span>
        <span v-else>1 parent &middot; <code>{{ commit.parents[0]?.slice(0, 8) }}</code></span>
        <span
          v-if="commit.committer.email !== commit.author.email"
          class="commit-details__committer"
          >Committed by {{ commit.committer.name }}</span
        >
      </p>
    </template>
  </GsCard>
</template>

<style scoped>
.commit-details__empty {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 0.82rem;
}

.commit-details__head {
  display: flex;
  align-items: flex-start;
  gap: 0.6rem;
}

.commit-details__subject {
  margin: 0;
  font-size: 0.88rem;
  font-weight: 650;
  color: var(--color-text);
}

.commit-details__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.4rem;
  margin: 0.5rem 0 0;
  font-size: 0.76rem;
  color: var(--color-text-muted);
}

.commit-details__meta code {
  color: var(--color-accent);
}

.commit-details__body {
  margin: 0.5rem 0 0;
  font-size: 0.8rem;
  line-height: 1.5;
  color: var(--color-text-muted);
  white-space: pre-wrap;
  /* A long commit body would otherwise push the footer out of the rail. */
  max-height: 7.5rem;
  overflow: auto;
}

.commit-details__badges {
  display: flex;
  flex-wrap: wrap;
  gap: 0.3rem;
  margin: 0.6rem 0 0;
}

.commit-details__badge {
  font-size: 0.69rem;
  font-weight: 600;
  padding: 0.08rem 0.4rem;
  border-radius: var(--radius-sm);
  color: #fff;
}

.commit-details__badge--branch {
  background: var(--color-badge-branch);
}
.commit-details__badge--head {
  background: var(--color-badge-neutral);
}
.commit-details__badge--tag {
  background: var(--color-lane-2);
  color: #1a1206;
}
.commit-details__badge--remote {
  background: var(--color-badge-alt);
}

.commit-details__footer {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  flex-wrap: wrap;
  margin: 0.7rem 0 0;
  padding-top: 0.6rem;
  border-top: 1px solid var(--color-border);
  font-size: 0.76rem;
  color: var(--color-text-muted);
}

.commit-details__committer {
  color: var(--color-text-faint);
}

.commit-details__menu-wrap {
  position: relative;
}

.commit-details__menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  min-width: 11rem;
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

.commit-details__menu button {
  justify-content: flex-start;
  text-align: left;
  width: 100%;
}
</style>
