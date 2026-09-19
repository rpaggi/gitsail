<script setup lang="ts">
// The Overview: a summary row, the commit list, and a right rail of
// detail cards — the layout the product mockup specifies as the landing
// screen.
//
// Every figure here comes from a store that already had it. Where a figure
// genuinely is not available (ahead/behind on a branch with no upstream,
// or before the branch list has loaded) the tile renders an em dash with a
// hint rather than a zero, because "0 ahead" and "we don't know yet" are
// very different facts to act on.

import { computed, onMounted } from "vue";

import CommitDetailsCard from "./CommitDetailsCard.vue";
import CommitListCard from "./CommitListCard.vue";
import FileChangesCard from "./FileChangesCard.vue";
import RecentActivityCard from "./RecentActivityCard.vue";
import StatCard from "./StatCard.vue";
import { useBranchesStore } from "../stores/branches";
import { useRepositorySessionStore } from "../stores/session";
import type { ViewId } from "./navigation";

const emit = defineEmits<{ (event: "navigate", id: ViewId): void }>();

const session = useRepositorySessionStore();
const branches = useBranchesStore();

const currentBranchName = computed(() => session.repository?.currentBranch ?? null);

const currentBranch = computed(() =>
  branches.branches.find(
    (branch) => branch.kind.kind === "local" && branch.name === currentBranchName.value,
  ) ?? null,
);

/** Ahead/behind, or `null` when the app cannot yet say. Three distinct
 * "no number" cases collapse here on purpose — not loaded, detached HEAD,
 * no upstream configured — because all three mean the same thing to the
 * tile: do not show a count. The hint below tells them apart in words. */
const tracking = computed(() => {
  const branch = currentBranch.value;
  if (!branch || branch.upstream === null) {
    return null;
  }
  return { ahead: branch.ahead, behind: branch.behind };
});

const trackingHint = computed(() => {
  if (tracking.value) {
    return `Compared with ${currentBranch.value?.upstream}`;
  }
  if (!currentBranchName.value) {
    return "HEAD is detached, so there is no branch to compare.";
  }
  if (branches.isLoading || branches.branches.length === 0) {
    return "Still reading the branch list.";
  }
  return "This branch has no upstream configured.";
});

const worktree = computed(() => {
  const status = session.status;
  if (!status) {
    return null;
  }
  return { isClean: status.isClean, count: status.files.length };
});

const worktreeValue = computed(() => {
  const value = worktree.value;
  if (value === null) {
    return "—";
  }
  return value.isClean ? "Clean" : `${value.count} changed`;
});

onMounted(() => {
  // The tiles need the branch list for ahead/behind; nothing else on this
  // screen would otherwise request it.
  if (branches.branches.length === 0 && !branches.isLoading) {
    void branches.load();
  }
});
</script>

<template>
  <div class="overview">
    <div class="overview__main">
      <div class="overview__stats">
        <StatCard
          icon="branch"
          :value="currentBranchName ?? 'detached'"
          label="Current branch"
          tone="accent"
          :hint="session.repository?.rootPath"
        />
        <StatCard
          icon="arrow-up"
          :value="tracking ? String(tracking.ahead) : '—'"
          label="Ahead"
          tone="accent"
          :hint="trackingHint"
        />
        <StatCard
          icon="arrow-down"
          :value="tracking ? String(tracking.behind) : '—'"
          label="Behind"
          tone="warning"
          :hint="trackingHint"
        />
        <StatCard
          :icon="worktree?.isClean === false ? 'alert' : 'check'"
          :value="worktreeValue"
          label="Working tree"
          :tone="worktree === null ? 'muted' : worktree.isClean ? 'success' : 'warning'"
          :hint="worktree === null ? 'Still reading the working tree.' : undefined"
        />
      </div>

      <CommitListCard />
    </div>

    <aside class="overview__rail" aria-label="Activity, file changes and commit details">
      <RecentActivityCard @view-all="emit('navigate', 'commits')" />
      <FileChangesCard @open-diff="emit('navigate', 'diff')" />
      <CommitDetailsCard />
    </aside>
  </div>
</template>

<style scoped>
.overview {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 380px;
  gap: 1rem;
  min-height: 0;
  flex: 1;
}

.overview__main {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-height: 0;
  min-width: 0;
}

.overview__stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.75rem;
  flex: none;
}

.overview__rail {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-height: 0;
  overflow-y: auto;
  padding-bottom: 2px;
}

/* The rail scrolls; its cards do not shrink. Without this the flex
   default (`shrink: 1`) compresses each card below its content and the
   card's own `overflow: hidden` silently clips the bottom of it — a
   footer line or the last activity entry just disappears. */
.overview__rail > * {
  flex: none;
}

/* The rail drops below the commit list rather than squeezing: at this
   width a 380px column would leave the graph too narrow to read a subject
   line, which is the whole point of the screen. */
@media (max-width: 82rem) {
  .overview {
    grid-template-columns: minmax(0, 1fr);
    overflow-y: auto;
  }
  .overview__main {
    min-height: 34rem;
  }
  .overview__rail {
    overflow: visible;
  }
}

@media (max-width: 52rem) {
  .overview__stats {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
