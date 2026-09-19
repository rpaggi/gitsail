<script setup lang="ts">
// The Desktop application shell (T-186/US-053), rebuilt against the
// product mockup (`assets/mockups/gitsail_gui_mockup.png`).
//
// Three regions: a top bar carrying the GitSail identity, the current
// repository and the global actions (`TopBar.vue`); a sidebar that is the
// primary view switcher (`SideNav.vue`); and a `<main>` that renders
// exactly one view at a time.
//
// This replaced the previous "everything at once" shell — a sidebar of
// stacked panels next to a graph and a three-tab strip. The panels
// themselves are unchanged and still own their own behavior; this file
// only decides *which* of them is on screen. The view registry lives in
// `navigation.ts` (pure, unit tested) rather than inline here, so "what
// the nav offers" and "what needs a repository" are testable facts rather
// than template details.
//
// Nothing that was reachable before became unreachable: Merge & Rebase
// moved into the Branches view (it operates on a branch, and shares the
// branches store), Amend into the Working Tree view (it rewrites the last
// commit from staged content), and the repository picker into Settings —
// which is also where a repository-scoped view falls back to when nothing
// is open, so a failed open is always recoverable.
//
// Shell-level empty/opening/error states (US-053 DoD) are still resolved
// by the pure `resolveShellState` (see `shellState.ts`) rather than
// inlined here, so that decision stays unit-tested directly.

import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import AmendPanel from "./AmendPanel.vue";
import BranchPanel from "./BranchPanel.vue";
import CommitDetailsCard from "./CommitDetailsCard.vue";
import CommitListCard from "./CommitListCard.vue";
import DiffViewer from "./DiffViewer.vue";
import GsCard from "./GsCard.vue";
import GsIcon from "./GsIcon.vue";
import KeybindingsPanel from "./KeybindingsPanel.vue";
import MergePanel from "./MergePanel.vue";
import OverviewView from "./OverviewView.vue";
import PullRequestsPanel from "./PullRequestsPanel.vue";
import ReferencesPanel from "./ReferencesPanel.vue";
import RecentRepositories from "./RecentRepositories.vue";
import RepositoryOpener from "./RepositoryOpener.vue";
import SideNav from "./SideNav.vue";
import StagingPanel from "./StagingPanel.vue";
import StatusPanel from "./StatusPanel.vue";
import SyncPanel from "./SyncPanel.vue";
import ThemeSwitcher from "./ThemeSwitcher.vue";
import TopBar from "./TopBar.vue";
import UpdateChecker from "./UpdateChecker.vue";
import { DEFAULT_VIEW, navItem, resolveView, type ViewId } from "./navigation";
import { resolveShellState } from "./shellState";
import { bindingFromKeyboardEvent } from "../keybindings";
import { useBlameStore } from "../stores/blame";
import { useDiffStore } from "../stores/diff";
import { useKeybindingsStore } from "../stores/keybindings";
import { useRepositorySessionStore } from "../stores/session";
import { useStagingStore } from "../stores/staging";
import { useSyncStore } from "../stores/sync";

const session = useRepositorySessionStore();

// -- T-249/US-107: global shortcut dispatch ------------------------------
//
// Maps each configurable action id to the real store call it already
// triggers elsewhere in the UI (`SyncPanel.vue`'s Fetch/Pull/Push buttons,
// `StagingPanel.vue`'s commit action) — this registry never invents a
// shortcut for something the UI cannot otherwise do. `focus-search` is the
// one exception with no store action: it just moves focus to
// `SearchPalette.vue`'s input (matched by id), which now lives in the top
// bar and so is reachable from every view.
const keybindings = useKeybindingsStore();
const sync = useSyncStore();
const staging = useStagingStore();
const diff = useDiffStore();
const blame = useBlameStore();

const GLOBAL_ACTION_HANDLERS: Record<string, () => void> = {
  "focus-search": () => {
    document.getElementById("gitsail-search-input")?.focus();
  },
  fetch: () => void sync.requestFetch(),
  pull: () => void sync.requestPull(),
  push: () => void sync.requestPush(),
  commit: () => void staging.requestCommit(),
};

/** Dispatches a global keydown to whichever configurable action currently
 * owns that binding (US-107 criterion 3: remapping an action changes what
 * actually fires here, never just what a settings panel displays). When
 * two actions share a binding (a detected conflict — see
 * `KeybindingsPanel.vue`), the first one in `CONFIGURABLE_ACTIONS`'
 * registration order wins, deterministically, rather than an undefined
 * "whichever handler happened to run". A remap capture in progress
 * (`KeybindingsPanel.vue`) always wins over this: its own listener is
 * registered in the capture phase and stops propagation, so this
 * bubble-phase listener never even sees that keydown. */
function onGlobalKeydown(event: KeyboardEvent): void {
  const binding = bindingFromKeyboardEvent(event);
  if (binding === null) {
    return;
  }
  for (const [actionId, effectiveBinding] of Object.entries(keybindings.bindings)) {
    if (effectiveBinding !== binding) {
      continue;
    }
    const handler = GLOBAL_ACTION_HANDLERS[actionId];
    if (handler) {
      event.preventDefault();
      handler();
    }
    return;
  }
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKeydown);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKeydown);
});

const shellState = computed(() =>
  resolveShellState({
    isOpening: session.isOpening,
    hasRepository: session.repository !== null,
    lastError: session.lastError,
  }),
);

// --- View switching -----------------------------------------------------
const selectedView = ref<ViewId>(DEFAULT_VIEW);

/** The view that actually renders. Routed through `resolveView` so a
 * repository-scoped selection with nothing open lands on Settings (where
 * the repository picker is) instead of on a dead screen. */
const activeView = computed(() => resolveView(selectedView.value, session.repository !== null));

const activeItem = computed(() => navItem(activeView.value));

function selectView(id: ViewId): void {
  selectedView.value = id;
}

// --- Blame entry points -------------------------------------------------
// `BlamePanel.vue` is an overlay opened for one file; it has no file
// picker of its own (it is normally reached from `DiffViewer.vue`). The
// Blame view is therefore a list of the files blame can currently be
// opened for — the working tree's changed files, plus whatever file the
// diff is scoped to — rather than an empty panel with no way in.
const blameCandidates = computed(() => {
  const paths = new Set<string>();
  if (diff.file) {
    paths.add(diff.file);
  }
  for (const file of session.status?.files ?? []) {
    paths.add(file.path);
  }
  return [...paths];
});
</script>

<template>
  <a href="#gitsail-main" class="skip-link">Skip to main content</a>

  <div class="app-shell">
    <TopBar />

    <SideNav :active="activeView" @select="selectView" />

    <main
      id="gitsail-main"
      class="app-shell__main"
      tabindex="-1"
      :aria-label="activeItem.title"
    >
      <template v-if="shellState.kind === 'opening'">
        <div class="app-shell__state" role="status" aria-live="polite">
          <GsIcon class="app-shell__state-icon app-shell__state-icon--spin" name="anchor" :size="38" />
          <p>Opening repository&hellip;</p>
        </div>
      </template>

      <template v-else-if="shellState.kind === 'error'">
        <div class="app-shell__state app-shell__state--error" role="alert">
          <GsIcon class="app-shell__state-icon" name="alert" :size="38" />
          <p>Could not open that repository: {{ shellState.message }}</p>
          <p v-if="shellState.remediation" class="app-shell__state-hint">{{ shellState.remediation }}</p>
          <div class="app-shell__state-action">
            <RepositoryOpener />
          </div>
        </div>
      </template>

      <template v-else>
        <header class="app-shell__page-head">
          <h1 class="app-shell__page-title">{{ activeItem.title }}</h1>
          <p class="app-shell__page-subtitle">{{ activeItem.subtitle }}</p>
        </header>

        <!--
          One view at a time, with `v-if`: unlike the previous tab strip,
          these are whole screens rather than sibling panels of one
          workspace, and keeping eleven of them mounted would mean every
          list and viewport in the app staying live behind whatever is on
          screen. Panel-local in-progress state that genuinely must
          survive a switch (a half-typed commit message, a loaded amend
          preview, an open rebase plan) already lives in a Pinia store,
          not in the component, so it does survive.
        -->
        <div class="app-shell__view">
          <OverviewView v-if="activeView === 'overview'" @navigate="selectView" />

          <div v-else-if="activeView === 'commits'" class="app-shell__split">
            <CommitListCard title="History" />
            <aside class="app-shell__rail" aria-label="Commit details">
              <CommitDetailsCard />
            </aside>
          </div>

          <div v-else-if="activeView === 'branches'" class="app-shell__stack">
            <GsCard title="Branches"><BranchPanel /></GsCard>
            <GsCard title="Merge &amp; Rebase"><MergePanel /></GsCard>
          </div>

          <GsCard v-else-if="activeView === 'stashes'" title="Stashes" fill>
            <ReferencesPanel initial-view="stash" />
          </GsCard>

          <GsCard v-else-if="activeView === 'pull-requests'" title="Pull &amp; Merge Requests" fill>
            <PullRequestsPanel />
          </GsCard>

          <!--
            Issues has no store, service or Core support in GitSail. It
            stays in the nav because the product intends it, and says so
            plainly rather than showing sample issues.
          -->
          <div v-else-if="activeView === 'issues'" class="app-shell__state">
            <GsIcon class="app-shell__state-icon" name="issue" :size="38" />
            <p class="app-shell__state-title">Issues aren't available yet</p>
            <p class="app-shell__state-hint">
              GitSail doesn't read issues from your forge yet. Pull requests are already here —
              issues are planned to follow.
            </p>
            <button type="button" class="btn-primary" @click="selectView('pull-requests')">
              Go to Pull Requests
            </button>
          </div>

          <div v-else-if="activeView === 'files'" class="app-shell__stack">
            <GsCard title="Status"><StatusPanel /></GsCard>
            <GsCard title="Stage &amp; Commit"><StagingPanel /></GsCard>
            <GsCard title="Amend last commit"><AmendPanel /></GsCard>
          </div>

          <GsCard v-else-if="activeView === 'diff'" title="Diff" fill>
            <DiffViewer />
          </GsCard>

          <GsCard v-else-if="activeView === 'blame'" title="Blame a file" fill>
            <p class="app-shell__hint">
              Blame opens for one file at a time. Pick one below, or open any file from the Diff
              view and choose "Blame" there.
            </p>
            <p v-if="blameCandidates.length === 0" class="app-shell__hint">
              No changed files right now — open a file from the Diff view to blame it.
            </p>
            <ul v-else class="app-shell__file-list">
              <li v-for="path in blameCandidates" :key="path">
                <button type="button" class="btn-ghost" @click="blame.open(path)">
                  <GsIcon name="blame" :size="14" />
                  {{ path }}
                </button>
              </li>
            </ul>
          </GsCard>

          <GsCard v-else-if="activeView === 'tags'" title="Tags" fill>
            <ReferencesPanel initial-view="tags" />
          </GsCard>

          <div v-else-if="activeView === 'remotes'" class="app-shell__stack">
            <GsCard title="Remotes"><ReferencesPanel initial-view="remotes" /></GsCard>
            <GsCard title="Sync"><SyncPanel /></GsCard>
          </div>

          <div v-else class="app-shell__settings">
            <GsCard title="Repository">
              <RepositoryOpener />
              <RecentRepositories />
            </GsCard>
            <GsCard title="Appearance"><ThemeSwitcher /></GsCard>
            <GsCard title="Keyboard shortcuts"><KeybindingsPanel /></GsCard>
            <GsCard title="Updates"><UpdateChecker /></GsCard>
          </div>
        </div>
      </template>
    </main>
  </div>
</template>

<style scoped>
/* A fixed-viewport grid rather than a scrolling document: the top bar and
   sidebar are chrome and must stay put while the content region — and
   only it — scrolls. */
.app-shell {
  display: grid;
  grid-template-areas:
    "topbar topbar"
    "sidenav main";
  grid-template-columns: auto minmax(0, 1fr);
  grid-template-rows: auto minmax(0, 1fr);
  height: 100vh;
  background: var(--color-bg);
}

.app-shell__main {
  grid-area: main;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
  padding: 1rem 1.15rem;
  overflow: hidden;
}

.app-shell__page-head {
  flex: none;
}

.app-shell__page-title {
  margin: 0;
  font-size: 1.3rem;
  font-weight: 700;
  letter-spacing: -0.015em;
  color: var(--color-text);
}

.app-shell__page-subtitle {
  margin: 0.15rem 0 0;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.app-shell__view {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.app-shell__split {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) 380px;
  gap: 1rem;
}

.app-shell__rail {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-height: 0;
  overflow-y: auto;
}

/* See `OverviewView.vue`: the rail scrolls, its cards keep their natural
   height rather than being compressed into their own `overflow: hidden`. */
.app-shell__rail > * {
  flex: none;
}

/* Stacked views scroll as a column of cards. */
.app-shell__stack,
.app-shell__settings {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  /* Room for the last card's shadow, which a flush `overflow` would clip. */
  padding-bottom: 2px;
}

/* The column scrolls; its cards keep their natural height. Without this
   the flex default (`shrink: 1`) squeezes each card into its own
   `overflow: hidden` and quietly cuts off its last rows. */
.app-shell__stack > *,
.app-shell__settings > * {
  flex: none;
}

.app-shell__settings {
  max-width: 46rem;
}

.app-shell__state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.5rem;
  text-align: center;
  padding: 2rem;
  color: var(--color-text-muted);
}

.app-shell__state--error {
  color: var(--color-danger);
}

.app-shell__state-icon {
  color: var(--color-text-faint);
}

.app-shell__state--error .app-shell__state-icon {
  color: var(--color-danger);
}

.app-shell__state-icon--spin {
  animation: gitsail-spin 1.6s linear infinite;
}

.app-shell__state-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 650;
  color: var(--color-text);
}

.app-shell__state-hint {
  margin: 0;
  font-size: 0.85rem;
  max-width: 30rem;
  color: var(--color-text-muted);
}

.app-shell__state-action {
  margin-top: 0.75rem;
  width: min(30rem, 100%);
  color: var(--color-text);
}

.app-shell__hint {
  margin: 0 0 0.6rem;
  font-size: 0.83rem;
  color: var(--color-text-muted);
}

.app-shell__file-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-shell__file-list button {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  width: 100%;
  text-align: left;
  font-family: var(--font-mono);
  font-size: 0.8rem;
}

@keyframes gitsail-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

/* Below this width a fixed sidebar column next to the content starts
   clipping actions; stack the regions instead and let the whole main area
   scroll. */
@media (max-width: 60rem) {
  .app-shell {
    grid-template-areas:
      "topbar"
      "sidenav"
      "main";
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto minmax(0, 1fr);
  }
  .app-shell__main {
    overflow-y: auto;
    padding: 0.85rem;
  }
  .app-shell__split {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
