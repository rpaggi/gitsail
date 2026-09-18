<script setup lang="ts">
// The consolidated Desktop layout (T-186/US-053) — replaces the flat,
// unstructured stack of panels `App.vue` mounted directly before this story
// (every existing panel component is reused as-is; this file only decides
// *where* each one lives). Three regions, per US-053 criterion 1:
//
//   - a header carrying the GitSail identity (US-053 criterion 2);
//   - a sidebar for repository selection, branches, remotes/sync, tags/
//     stash, and pull/merge requests (criterion 1);
//   - a main area with the commit graph central and a tabbed area below it
//     for changes/merge-rebase/amend (criterion 1, "possivelmente com
//     abas").
//
// Sidebar scope: US-053 criterion 1 lists "branches/remotes/tags/stashes"
// for the sidebar. As of this story (T-186/US-053) only branches
// (`BranchPanel`), remotes (`SyncPanel`) and pull/merge requests
// (`PullRequestsPanel`) existed as components — there was no tags or
// stashes panel/store/service yet. `ReferencesPanel` (T-195/US-062) closes
// that gap, mirroring `gitsail-tui`'s own `Panel::References` semantics
// (tags/remotes/stash together, with sub-views) rather than three separate
// sidebar sections.
//
// Shell-level empty/opening/error states (US-053 DoD) are resolved by the
// pure `resolveShellState` (see `shellState.ts`) rather than inlined here,
// so that decision is unit-tested directly. Only in the `ready` state does
// the sidebar's repository-scoped sections and the main workspace render —
// `RepositoryOpener`/`RecentRepositories` (the one way to *reach* `ready`)
// are always present regardless of state.

import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";

import AmendPanel from "./AmendPanel.vue";
import BranchPanel from "./BranchPanel.vue";
import CommitGraph from "./CommitGraph.vue";
import DiffViewer from "./DiffViewer.vue";
import KeybindingsPanel from "./KeybindingsPanel.vue";
import MergePanel from "./MergePanel.vue";
import PullRequestsPanel from "./PullRequestsPanel.vue";
import ReferencesPanel from "./ReferencesPanel.vue";
import RecentRepositories from "./RecentRepositories.vue";
import RepositoryOpener from "./RepositoryOpener.vue";
import SearchPalette from "./SearchPalette.vue";
import StagingPanel from "./StagingPanel.vue";
import StatusPanel from "./StatusPanel.vue";
import SyncPanel from "./SyncPanel.vue";
import ThemeSwitcher from "./ThemeSwitcher.vue";
import { rovingNextIndex } from "./keyboardNav";
import { resolveShellState } from "./shellState";
import { bindingFromKeyboardEvent } from "../keybindings";
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
// `SearchPalette.vue`'s input (matched by id).
const keybindings = useKeybindingsStore();
const sync = useSyncStore();
const staging = useStagingStore();

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

/** The repository's own folder name, for the header breadcrumb — derived
 * rather than stored, so it can never drift from `session.repository`
 * (US-054's own single-source-of-truth session already owns that value;
 * this only formats it). Handles both `/` and `\` so a Windows root path
 * still yields a sensible name (US-030-era path-handling convention: never
 * assume a path separator). */
const repositoryName = computed<string | null>(() => {
  const root = session.repository?.rootPath;
  if (!root) {
    return null;
  }
  const segments = root.split(/[/\\]+/).filter((segment) => segment.length > 0);
  return segments.length > 0 ? segments[segments.length - 1] : root;
});

// --- Main-area tabs (US-053 criterion 1: "possivelmente com abas") -------
// WAI-ARIA "tabs" pattern: a horizontal, wrapping roving-tabindex list
// (US-055 criterion 1). Panels stay mounted (`v-show`, not `v-if`) switching
// tabs never discards a panel's own in-progress state (a half-typed commit
// message, an open rebase plan, an amend preview already loaded).
interface ShellTab {
  id: string;
  label: string;
  description: string;
}

const tabs: ShellTab[] = [
  {
    id: "changes",
    label: "Changes",
    description: "Stage files, write a commit message, and inspect diffs.",
  },
  {
    id: "merge",
    label: "Merge & Rebase",
    description: "Merge or rebase a branch in, resolve conflicts, and continue/abort.",
  },
  {
    id: "amend",
    label: "Amend",
    description: "Rewrite the last commit's message and/or staged content.",
  },
];

const activeTabIndex = ref(0);
const tabButtonEls = ref<(HTMLButtonElement | null)[]>([]);

function setTabButtonRef(el: Element | null, index: number): void {
  tabButtonEls.value[index] = el as HTMLButtonElement | null;
}

function selectTab(index: number): void {
  activeTabIndex.value = index;
}

/** Arrow/Home/End roving-tabindex navigation for the tablist (US-055
 * criterion 1). Only the active tab is ever in the normal tab order
 * (`tabindex="0"`); every other tab is `-1` and reached by arrow key, per
 * the WAI-ARIA APG tabs pattern — this is what lets Tab itself skip straight
 * from the tablist to the active panel's own controls instead of stopping
 * on all three tab buttons. */
function onTabKeydown(event: KeyboardEvent, index: number): void {
  const next = rovingNextIndex(index, event.key, tabs.length, "horizontal");
  if (next === null) {
    return;
  }
  event.preventDefault();
  activeTabIndex.value = next;
  void nextTick(() => {
    tabButtonEls.value[next]?.focus();
  });
}
</script>

<template>
  <a href="#gitsail-main" class="skip-link">Skip to main content</a>

  <div class="app-shell">
    <header class="app-shell__header">
      <div class="app-shell__brand">
        <img
          class="app-shell__logo"
          src="/branding/logo_gitsail.png"
          alt="GitSail — nautical mascot logo"
          width="40"
          height="40"
        />
        <svg
          class="app-shell__sail-mark"
          viewBox="0 0 24 24"
          aria-hidden="true"
          focusable="false"
        >
          <path d="M12 3 L12 18" class="app-shell__sail-mast" />
          <path d="M12 4 L18.5 15 L12 15 Z" class="app-shell__sail-cloth" />
          <path d="M4 20 Q12 17 20 20" class="app-shell__sail-wave" />
        </svg>
        <div class="app-shell__wordmark">
          <h1 class="app-shell__title">GitSail</h1>
          <p class="app-shell__tagline">Navigate your Git history.</p>
        </div>
      </div>

      <p v-if="repositoryName" class="app-shell__repo" aria-live="polite">
        <span class="app-shell__repo-label">Repository:</span>
        <strong>{{ repositoryName }}</strong>
        <span v-if="session.repository?.currentBranch" class="app-shell__repo-branch">
          on <code>{{ session.repository.currentBranch }}</code>
        </span>
      </p>
    </header>

    <div class="app-shell__body">
      <aside class="app-shell__sidebar" aria-label="Branches, remotes, references and pull requests">
        <section class="app-shell__section">
          <h2 class="app-shell__section-title">Repository</h2>
          <RepositoryOpener />
          <RecentRepositories />
        </section>

        <!--
          Settings (T-248/US-106, T-249/US-107): always visible regardless
          of `shellState` — theme and keyboard shortcuts are app-global
          preferences, never scoped to whichever repository (if any) is
          currently open.
        -->
        <section class="app-shell__section">
          <h2 class="app-shell__section-title">Settings</h2>
          <ThemeSwitcher />
          <details class="app-shell__shortcuts">
            <summary>Keyboard shortcuts</summary>
            <KeybindingsPanel />
          </details>
        </section>

        <template v-if="shellState.kind === 'ready'">
          <section class="app-shell__section">
            <h2 class="app-shell__section-title">Search</h2>
            <SearchPalette />
          </section>
          <section class="app-shell__section">
            <BranchPanel />
          </section>
          <section class="app-shell__section">
            <SyncPanel />
          </section>
          <section class="app-shell__section">
            <ReferencesPanel />
          </section>
          <section class="app-shell__section">
            <PullRequestsPanel />
          </section>
        </template>
      </aside>

      <main id="gitsail-main" class="app-shell__main" tabindex="-1" aria-label="Commit graph and changes">
        <template v-if="shellState.kind === 'opening'">
          <div class="app-shell__state" role="status" aria-live="polite">
            <span class="app-shell__state-icon app-shell__state-icon--spin" aria-hidden="true">&#9875;</span>
            <p>Opening repository&hellip;</p>
          </div>
        </template>

        <template v-else-if="shellState.kind === 'empty'">
          <div class="app-shell__state">
            <span class="app-shell__state-icon" aria-hidden="true">&#8985;</span>
            <p>No repository open yet.</p>
            <p class="app-shell__state-hint">
              Use "Browse&hellip;" or pick a recent repository in the sidebar to get started.
            </p>
          </div>
        </template>

        <template v-else-if="shellState.kind === 'error'">
          <div class="app-shell__state app-shell__state--error" role="alert">
            <span class="app-shell__state-icon" aria-hidden="true">&#9888;</span>
            <p>Could not open that repository: {{ shellState.message }}</p>
            <p v-if="shellState.remediation" class="app-shell__state-hint">{{ shellState.remediation }}</p>
          </div>
        </template>

        <template v-else>
          <StatusPanel />

          <section class="app-shell__graph" aria-label="Commit graph">
            <CommitGraph />
          </section>

          <section class="app-shell__tabs">
            <div class="app-shell__tablist" role="tablist" aria-label="Changes, merge/rebase and amend">
              <button
                v-for="(tab, index) in tabs"
                :key="tab.id"
                :ref="(el) => setTabButtonRef(el as Element | null, index)"
                role="tab"
                type="button"
                :id="`gitsail-tab-${tab.id}`"
                :aria-selected="activeTabIndex === index"
                :aria-controls="`gitsail-tabpanel-${tab.id}`"
                :tabindex="activeTabIndex === index ? 0 : -1"
                :title="tab.description"
                @click="selectTab(index)"
                @keydown="onTabKeydown($event, index)"
              >
                {{ tab.label }}
              </button>
            </div>

            <div
              v-show="activeTabIndex === 0"
              :id="`gitsail-tabpanel-changes`"
              role="tabpanel"
              aria-labelledby="gitsail-tab-changes"
              tabindex="0"
              class="app-shell__tabpanel"
            >
              <StagingPanel />
              <DiffViewer />
            </div>

            <div
              v-show="activeTabIndex === 1"
              :id="`gitsail-tabpanel-merge`"
              role="tabpanel"
              aria-labelledby="gitsail-tab-merge"
              tabindex="0"
              class="app-shell__tabpanel"
            >
              <MergePanel />
            </div>

            <div
              v-show="activeTabIndex === 2"
              :id="`gitsail-tabpanel-amend`"
              role="tabpanel"
              aria-labelledby="gitsail-tab-amend"
              tabindex="0"
              class="app-shell__tabpanel"
            >
              <AmendPanel />
            </div>
          </section>
        </template>
      </main>
    </div>

    <footer class="app-shell__footer">
      <span>GitSail — Navigate your Git history.</span>
    </footer>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

.app-shell__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  flex-wrap: wrap;
  padding: 0.75rem 1.25rem;
  background: linear-gradient(135deg, var(--color-surface) 0%, var(--color-bg) 100%);
  border-bottom: 1px solid var(--color-border);
}

.app-shell__brand {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  min-width: 0;
}

.app-shell__logo {
  width: 40px;
  height: 40px;
  border-radius: 6px;
  flex-shrink: 0;
}

.app-shell__sail-mark {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
}
.app-shell__sail-mast {
  stroke: var(--color-text-muted);
  stroke-width: 1.5;
  fill: none;
}
.app-shell__sail-cloth {
  fill: var(--color-accent);
  opacity: 0.9;
}
.app-shell__sail-wave {
  stroke: var(--color-accent-strong);
  stroke-width: 1.5;
  fill: none;
}

.app-shell__wordmark {
  min-width: 0;
}
.app-shell__title {
  margin: 0;
  font-size: 1.25rem;
  line-height: 1.1;
}
.app-shell__tagline {
  margin: 0;
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.app-shell__repo {
  margin: 0;
  font-size: 0.85rem;
  color: var(--color-text-muted);
  display: flex;
  gap: 0.35rem;
  align-items: baseline;
  flex-wrap: wrap;
}
.app-shell__repo strong {
  color: var(--color-text);
}
.app-shell__repo-branch code {
  font-family: var(--font-mono);
}

.app-shell__body {
  flex: 1;
  display: flex;
  gap: 1rem;
  padding: 1rem 1.25rem;
  min-height: 0;
}

.app-shell__sidebar {
  width: 20rem;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  overflow-y: auto;
}

.app-shell__section {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 0.75rem;
}
.app-shell__section-title {
  margin: 0 0 0.5rem;
  font-size: 0.85rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}

.app-shell__shortcuts {
  margin-top: 0.75rem;
}
.app-shell__shortcuts summary {
  cursor: pointer;
  color: var(--color-text-muted);
}

.app-shell__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.app-shell__graph {
  height: 40vh;
  min-height: 16rem;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  overflow: hidden;
}

.app-shell__tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  overflow: hidden;
}

.app-shell__tablist {
  display: flex;
  gap: 0.25rem;
  padding: 0.5rem 0.5rem 0;
  border-bottom: 1px solid var(--color-border);
}
.app-shell__tablist button {
  background: none;
  border: none;
  padding: 0.5rem 0.9rem;
  color: var(--color-text-muted);
  cursor: pointer;
  border-bottom: 2px solid transparent;
}
.app-shell__tablist button[aria-selected="true"] {
  color: var(--color-text);
  border-bottom-color: var(--color-accent);
  font-weight: 600;
}

.app-shell__tabpanel {
  flex: 1;
  overflow: auto;
  padding: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.app-shell__state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.35rem;
  text-align: center;
  padding: 2rem;
  color: var(--color-text-muted);
}
.app-shell__state--error {
  color: var(--color-danger);
}
.app-shell__state-icon {
  font-size: 2rem;
  line-height: 1;
}
.app-shell__state-icon--spin {
  display: inline-block;
  animation: gitsail-spin 1.6s linear infinite;
}
.app-shell__state-hint {
  font-size: 0.85rem;
  max-width: 28rem;
}

@keyframes gitsail-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.app-shell__footer {
  padding: 0.4rem 1.25rem;
  border-top: 1px solid var(--color-border);
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

/* Responsive fallback (US-055 criterion 2): below this width a fixed
   20rem sidebar next to a graph/tabs column would start clipping actions.
   Stack sidebar above main instead, and let the sidebar's own height be
   content-driven (no forced min-height) with an internal scrollbar. */
@media (max-width: 60rem) {
  .app-shell__body {
    flex-direction: column;
  }
  .app-shell__sidebar {
    width: auto;
    max-height: 40vh;
  }
}
</style>
