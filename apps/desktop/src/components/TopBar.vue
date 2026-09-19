<script setup lang="ts">
// The application top bar: brand, current repository, branch switcher,
// global search, and the sync actions.
//
// Every control here is a thin shell over an action that already existed
// elsewhere in the UI — the branch pill calls the same
// `branches.requestSwitch` the Branches view does, and the Sync menu's
// three items are `SyncPanel.vue`'s own Fetch/Pull/Push. Nothing in this
// bar is a new capability, so nothing here can bypass the confirmation
// flow `stores/operation.ts` puts in front of a mutating action.

import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import GsIcon from "./GsIcon.vue";
import SearchPalette from "./SearchPalette.vue";
import { useBranchesStore } from "../stores/branches";
import { useRepositorySessionStore } from "../stores/session";
import { useSyncStore } from "../stores/sync";

const session = useRepositorySessionStore();
const branches = useBranchesStore();
const sync = useSyncStore();

/** The repository's own folder name, for the header breadcrumb — derived
 * rather than stored, so it can never drift from `session.repository`
 * (`stores/session.ts` already owns that value; this only formats it).
 * Handles both `/` and `\` so a Windows root path still yields a sensible
 * name (never assume a path separator). */
const repositoryName = computed<string | null>(() => {
  const root = session.repository?.rootPath;
  if (!root) {
    return null;
  }
  const segments = root.split(/[/\\]+/).filter((segment) => segment.length > 0);
  return segments.length > 0 ? segments[segments.length - 1] : root;
});

const localBranches = computed(() =>
  branches.branches.filter((branch) => branch.kind.kind === "local"),
);

/** The branch HEAD is actually on. Read from the session (the single
 * source of truth for HEAD) rather than from the branches list, which may
 * not have loaded yet — the pill should show the real branch immediately,
 * not "(none)" until a second request lands. */
const currentBranch = computed(() => session.repository?.currentBranch ?? null);

/** Ahead/behind for the current branch. This is the *only* place in the
 * DTOs carrying those counts (`BranchDto`), so it is absent until the
 * branches list loads, and absent entirely on a detached HEAD — both
 * render as nothing rather than as zeros, which would claim "in sync"
 * without knowing it. */
const tracking = computed(() => {
  const name = currentBranch.value;
  if (!name) {
    return null;
  }
  const branch = localBranches.value.find((candidate) => candidate.name === name);
  if (!branch || branch.upstream === null) {
    return null;
  }
  return { ahead: branch.ahead, behind: branch.behind };
});

function onBranchChange(event: Event): void {
  const target = event.target as HTMLSelectElement;
  if (target.value && target.value !== currentBranch.value) {
    void branches.requestSwitch(target.value);
  }
}

// --- Sync menu ---------------------------------------------------------
// A menu rather than one "Sync" button that fetches-pulls-pushes in
// sequence: the stores expose the three as separate, separately-confirmed
// operations, and inventing a compound one here would put a mutation
// behind a button whose label does not say what it will do.
const syncMenuOpen = ref(false);
const syncMenuEl = ref<HTMLElement | null>(null);

const syncActions = [
  { id: "fetch", label: "Fetch", icon: "arrow-down", run: () => sync.requestFetch() },
  { id: "pull", label: "Pull", icon: "arrow-down", run: () => sync.requestPull() },
  { id: "push", label: "Push", icon: "arrow-up", run: () => sync.requestPush() },
];

function toggleSyncMenu(): void {
  syncMenuOpen.value = !syncMenuOpen.value;
}

function runSyncAction(action: (typeof syncActions)[number]): void {
  syncMenuOpen.value = false;
  void action.run();
}

/** Closes the menu on any click outside it — registered on `document` only
 * while the menu is open, so the app is not paying for a global listener
 * for the 99% of the time the menu is shut. */
function onDocumentClick(event: MouseEvent): void {
  if (syncMenuOpen.value && !syncMenuEl.value?.contains(event.target as Node)) {
    syncMenuOpen.value = false;
  }
}

onMounted(() => document.addEventListener("click", onDocumentClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick));
</script>

<template>
  <header class="topbar">
    <div class="topbar__brand">
      <img
        class="topbar__logo"
        src="/branding/logo_gitsail.png"
        alt=""
        width="26"
        height="26"
      />
      <span class="topbar__wordmark">GitSail</span>
    </div>

    <template v-if="session.repository">
      <div class="topbar__repo" :title="session.repository.rootPath">
        <GsIcon name="repo" :size="15" />
        <span class="topbar__repo-name">{{ repositoryName }}</span>
      </div>

      <div class="topbar__branch">
        <GsIcon name="branch" :size="14" />
        <label class="sr-only" for="gitsail-branch-select">Current branch</label>
        <select
          id="gitsail-branch-select"
          class="topbar__branch-select"
          :value="currentBranch ?? ''"
          @change="onBranchChange"
        >
          <option v-if="!currentBranch" value="">(detached)</option>
          <option v-for="branch in localBranches" :key="branch.name" :value="branch.name">
            {{ branch.name }}
          </option>
        </select>
        <GsIcon class="topbar__branch-chevron" name="chevron" :size="14" />
      </div>

      <p v-if="tracking" class="topbar__tracking">
        <span v-if="tracking.ahead > 0" class="topbar__tracking-item">
          <GsIcon name="arrow-up" :size="12" />{{ tracking.ahead }}
          <span class="sr-only">commits ahead of upstream</span>
        </span>
        <span v-if="tracking.behind > 0" class="topbar__tracking-item">
          <GsIcon name="arrow-down" :size="12" />{{ tracking.behind }}
          <span class="sr-only">commits behind upstream</span>
        </span>
      </p>
    </template>

    <div class="topbar__search">
      <SearchPalette />
    </div>

    <div class="topbar__actions">
      <button
        type="button"
        class="btn-icon btn-ghost"
        aria-label="Refresh repository status"
        title="Refresh repository status"
        :disabled="!session.repository || session.isRefreshing"
        @click="session.refreshStatus('manual')"
      >
        <GsIcon name="refresh" :size="16" />
      </button>

      <button
        v-if="sync.forgeLink"
        type="button"
        class="btn-icon btn-ghost"
        aria-label="Open this repository in your browser"
        title="Open this repository in your browser"
        @click="sync.openRepositoryForgeLink()"
      >
        <GsIcon name="external" :size="16" />
      </button>

      <div ref="syncMenuEl" class="topbar__sync">
        <button
          type="button"
          class="btn-primary topbar__sync-button"
          :disabled="!session.repository"
          aria-haspopup="menu"
          :aria-expanded="syncMenuOpen"
          @click="toggleSyncMenu"
        >
          <GsIcon name="sync" :size="15" />
          Sync
          <GsIcon name="chevron" :size="13" />
        </button>
        <div v-if="syncMenuOpen" class="topbar__sync-menu" role="menu" aria-label="Sync actions">
          <button
            v-for="action in syncActions"
            :key="action.id"
            type="button"
            role="menuitem"
            class="btn-ghost topbar__sync-item"
            :disabled="sync.resolveError !== null"
            @click="runSyncAction(action)"
          >
            <GsIcon :name="action.icon" :size="14" />
            {{ action.label }}
          </button>
          <p v-if="sync.resolveError" class="topbar__sync-error">
            {{ sync.resolveError.message }}
          </p>
        </div>
      </div>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  grid-area: topbar;
  display: flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0 0.85rem;
  height: 52px;
  background: var(--color-chrome);
  border-bottom: 1px solid var(--color-border);
  /* Above the main content so the search dropdown, which escapes this bar,
     is never clipped by a card below it. */
  position: relative;
  z-index: 40;
}

.topbar__brand {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  flex: none;
}

.topbar__logo {
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
}

.topbar__wordmark {
  font-size: 0.98rem;
  font-weight: 700;
  letter-spacing: -0.01em;
}

.topbar__repo,
.topbar__branch {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex: none;
  color: var(--color-text-muted);
}

.topbar__repo {
  padding-left: 0.5rem;
  border-left: 1px solid var(--color-border);
  margin-left: 0.2rem;
}

.topbar__repo-name {
  color: var(--color-text);
  font-weight: 600;
  max-width: 13rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The mockup's branch "pill". The native `<select>` keeps its own keyboard
   and screen-reader behavior; only its chrome is replaced, with the
   chevron drawn beside it and the select's own arrow suppressed. */
.topbar__branch {
  padding: 0 0.5rem;
  height: 30px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-surface-raised);
}

.topbar__branch-select {
  appearance: none;
  background: transparent;
  border: none;
  padding: 0 0.1rem;
  height: 100%;
  color: var(--color-text);
  font-weight: 600;
  cursor: pointer;
  max-width: 11rem;
}

.topbar__branch-chevron {
  color: var(--color-text-faint);
  pointer-events: none;
}

.topbar__tracking {
  display: flex;
  gap: 0.4rem;
  margin: 0;
  flex: none;
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.topbar__tracking-item {
  display: inline-flex;
  align-items: center;
  gap: 0.15rem;
}

.topbar__search {
  flex: 1;
  min-width: 0;
  max-width: 34rem;
  /* Centered in the leftover space, as in the mockup, rather than pinned
     to whatever width the chips to its left happen to leave. */
  margin: 0 auto;
}

.topbar__actions {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  flex: none;
}

.topbar__sync {
  position: relative;
}

.topbar__sync-button {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  height: 32px;
}

.topbar__sync-menu {
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
  z-index: 70;
}

.topbar__sync-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  justify-content: flex-start;
  width: 100%;
}

.topbar__sync-error {
  margin: 0.25rem;
  font-size: 0.78rem;
  color: var(--color-danger);
}

/* Below this width the repo/branch chips and the search bar cannot all
   hold their minimum size on one row; the chips are the ones that can be
   recovered from the sidebar's repository card, so they go first. */
@media (max-width: 60rem) {
  .topbar__repo,
  .topbar__branch,
  .topbar__tracking {
    display: none;
  }
}
</style>
