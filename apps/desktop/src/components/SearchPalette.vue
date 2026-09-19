<script setup lang="ts">
// Global commit/branch search (US-056). Selecting a commit result shares
// identity with the commit graph (`stores/search.ts`'s `selectCommit`
// calls straight into `stores/graph.ts`) — criterion 2's "graph/lista/
// detalhes preservam identidade da seleção". The actions row below a
// selected commit is this story's command palette: only actions valid for
// a commit (copy its hash, branch from it) are offered, never a generic
// action list.

import { computed, ref } from "vue";

import GsIcon from "./GsIcon.vue";
import { useSearchStore } from "../stores/search";
import { useCommitGraphStore } from "../stores/graph";
import { useBranchesStore } from "../stores/branches";
import { useKeybindingsStore } from "../stores/keybindings";
import { formatBindingForDisplay } from "../keybindings";
import type { CommitDto } from "../services/dto";

const search = useSearchStore();
const graph = useCommitGraphStore();
const branches = useBranchesStore();
const keybindings = useKeybindingsStore();

const queryInput = ref("");

// T-249/US-107 criterion 3: this shows whatever "focus-search" is
// *currently* bound to (its default, or a person's own remap) — never a
// hardcoded default, so remapping it in `KeybindingsPanel.vue` is
// reflected here immediately.
const focusSearchBinding = computed(() => formatBindingForDisplay(keybindings.bindings["focus-search"]));

function runSearch(): void {
  void search.search(queryInput.value);
}

function selectCommit(commit: CommitDto): void {
  search.selectCommit(commit.hash);
}

function copyHash(hash: string): void {
  void navigator.clipboard?.writeText(hash);
}

function createBranchHere(hash: string): void {
  const name = window.prompt("New branch name, starting at " + hash.slice(0, 8));
  if (name && name.trim().length > 0) {
    void branches.requestCreate(name.trim(), hash);
  }
}

// Whether the results dropdown should be on screen at all. It overlays the
// page, so it must collapse the moment there is nothing to show — an empty
// floating panel would sit on top of the content for no reason.
const hasResults = computed(
  () =>
    search.exactMatch !== null ||
    search.branchResults.length > 0 ||
    search.commitResults.length > 0,
);

/** Clears the query and the results, returning the top bar to its resting
 * state. Bound to Escape as well as the clear button, since the dropdown
 * covers content and Escape is the universal "put that away". */
function clear(): void {
  queryInput.value = "";
  search.clear();
}
</script>

<template>
  <div class="search-palette" @keydown.esc="clear">
    <label for="gitsail-search-input" class="sr-only">Search commits and branches</label>
    <div class="search-palette__input-row">
      <GsIcon class="search-palette__icon" name="search" :size="15" />
      <input
        id="gitsail-search-input"
        v-model="queryInput"
        type="text"
        placeholder="Search commits, files, branches…"
        autocomplete="off"
        spellcheck="false"
        role="combobox"
        aria-controls="gitsail-search-results"
        :aria-expanded="hasResults"
        @input="runSearch"
        @keyup.enter="runSearch"
      />
      <button
        v-if="queryInput.length > 0"
        type="button"
        class="search-palette__clear"
        aria-label="Clear search"
        @click="clear"
      >
        &times;
      </button>
      <kbd v-else class="search-palette__shortcut" :title="`Shortcut: ${focusSearchBinding}`">{{ focusSearchBinding }}</kbd>
    </div>

    <!--
      Results overlay the page rather than pushing it down: the search box
      lives in the top bar now, so inline results would shove the entire
      workspace on every keystroke.
    -->
    <div
      v-if="hasResults || search.lastError"
      id="gitsail-search-results"
      class="search-palette__results"
    >
      <p v-if="search.lastError" class="error">{{ search.lastError.message }}</p>

      <div v-if="search.exactMatch" class="search-palette__exact-match">
        <strong>Exact match</strong>
        <div class="search-palette__result" :class="{ selected: graph.selectedHash === search.exactMatch.hash }">
          <button
            class="search-palette__result-trigger"
            type="button"
            :aria-label="`Select commit ${search.exactMatch.shortHash}`"
            @click="selectCommit(search.exactMatch)"
          >
            <code>{{ search.exactMatch.shortHash }}</code> {{ search.exactMatch.subject }}
          </button>
        </div>
      </div>

      <div v-if="search.branchResults.length > 0" class="search-palette__section">
        <strong>Branches</strong>
        <ul>
          <li v-for="branch in search.branchResults" :key="branch.name">
            <span>{{ branch.name }}</span>
            <button :aria-label="`Switch to ${branch.name}`" @click="branches.requestSwitch(branch.name)">Switch</button>
          </li>
        </ul>
      </div>

      <div v-if="search.commitResults.length > 0" class="search-palette__section">
        <strong>Commits</strong>
        <ul>
          <li
            v-for="commit in search.commitResults"
            :key="commit.hash"
            :class="{ selected: graph.selectedHash === commit.hash }"
          >
            <button
              class="search-palette__result-trigger"
              type="button"
              :aria-label="`Select commit ${commit.shortHash}`"
              @click="selectCommit(commit)"
            >
              <code>{{ commit.shortHash }}</code> {{ commit.subject }}
            </button>
            <span v-if="graph.selectedHash === commit.hash" class="search-palette__actions">
              <button :aria-label="`Copy hash ${commit.shortHash}`" @click="copyHash(commit.hash)">Copy hash</button>
              <button :aria-label="`Create branch at ${commit.shortHash}`" @click="createBranchHere(commit.hash)">Branch here…</button>
            </span>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>

<style scoped>
.search-palette {
  position: relative;
  width: 100%;
}

/* The whole row is the input's visual chrome; the real `<input>` inside is
   transparent and borderless, so the icon and the shortcut chip can sit
   inside the rounded field instead of beside it. */
.search-palette__input-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0 0.6rem;
  height: 34px;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-surface-raised);
}

.search-palette__input-row:focus-within {
  border-color: var(--color-accent);
}

.search-palette__icon {
  color: var(--color-text-faint);
}

.search-palette input {
  flex: 1;
  min-width: 0;
  /* Overrides the global input chrome — this one input is styled by its
     wrapper row instead, so it must not draw a second border inside it. */
  background: transparent;
  border: none;
  padding: 0;
  height: 100%;
}

.search-palette input:focus-visible {
  outline: none;
}

.search-palette__shortcut {
  flex-shrink: 0;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  padding: 0.05rem 0.35rem;
  font-family: var(--font-mono);
  font-size: 0.72rem;
  color: var(--color-text-muted);
}

.search-palette__clear {
  flex: none;
  width: 20px;
  height: 20px;
  padding: 0;
  line-height: 1;
  font-size: 1rem;
  border-radius: 50%;
  border: none;
  background: var(--color-surface-alt);
  color: var(--color-text-muted);
  cursor: pointer;
}

.search-palette__results {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  right: 0;
  z-index: 60;
  max-height: 60vh;
  overflow: auto;
  padding: 0.5rem;
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-pop);
}

.search-palette__section,
.search-palette__exact-match {
  margin-top: 0.35rem;
}

.search-palette__section > strong,
.search-palette__exact-match > strong {
  display: block;
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-faint);
  margin-bottom: 0.2rem;
}

.search-palette ul {
  list-style: none;
  margin: 0;
  padding: 0;
}

.search-palette li,
.search-palette__result {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0.4rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.search-palette li:hover,
.search-palette__result:hover {
  background: var(--color-surface-raised);
}

.search-palette li.selected,
.search-palette__result.selected {
  background: var(--color-accent-soft);
}

.search-palette code {
  color: var(--color-accent);
  margin-right: 0.35rem;
}

.search-palette__actions {
  display: flex;
  gap: 0.25rem;
}

.search-palette__result-trigger {
  background: none;
  border: none;
  color: inherit;
  font: inherit;
  text-align: left;
  padding: 0;
  cursor: pointer;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
