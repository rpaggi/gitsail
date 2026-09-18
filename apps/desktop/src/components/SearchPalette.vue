<script setup lang="ts">
// Global commit/branch search (US-056). Selecting a commit result shares
// identity with the commit graph (`stores/search.ts`'s `selectCommit`
// calls straight into `stores/graph.ts`) — criterion 2's "graph/lista/
// detalhes preservam identidade da seleção". The actions row below a
// selected commit is this story's command palette: only actions valid for
// a commit (copy its hash, branch from it) are offered, never a generic
// action list.

import { ref } from "vue";

import { useSearchStore } from "../stores/search";
import { useCommitGraphStore } from "../stores/graph";
import { useBranchesStore } from "../stores/branches";
import type { CommitDto } from "../services/dto";

const search = useSearchStore();
const graph = useCommitGraphStore();
const branches = useBranchesStore();

const queryInput = ref("");

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
</script>

<template>
  <div class="search-palette">
    <input
      v-model="queryInput"
      type="text"
      placeholder="Search commits (hash, message, author) or branches…"
      @input="runSearch"
      @keyup.enter="runSearch"
    />
    <p v-if="search.lastError" class="error">{{ search.lastError.message }}</p>

    <div v-if="search.exactMatch" class="search-palette__exact-match">
      <strong>Exact match</strong>
      <div class="search-palette__result" :class="{ selected: graph.selectedHash === search.exactMatch.hash }">
        <span @click="selectCommit(search.exactMatch)">
          <code>{{ search.exactMatch.shortHash }}</code> {{ search.exactMatch.subject }}
        </span>
      </div>
    </div>

    <div v-if="search.branchResults.length > 0" class="search-palette__section">
      <strong>Branches</strong>
      <ul>
        <li v-for="branch in search.branchResults" :key="branch.name">
          <span>{{ branch.name }}</span>
          <button @click="branches.requestSwitch(branch.name)">Switch</button>
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
          <span @click="selectCommit(commit)">
            <code>{{ commit.shortHash }}</code> {{ commit.subject }}
          </span>
          <span v-if="graph.selectedHash === commit.hash" class="search-palette__actions">
            <button @click="copyHash(commit.hash)">Copy hash</button>
            <button @click="createBranchHere(commit.hash)">Branch here…</button>
          </span>
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.search-palette input {
  width: 100%;
  box-sizing: border-box;
}
.search-palette__section,
.search-palette__exact-match {
  margin-top: 0.5rem;
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
  padding: 0.15rem 0;
  cursor: pointer;
}
.search-palette li.selected,
.search-palette__result.selected {
  background: rgba(100, 180, 255, 0.18);
}
.search-palette__actions {
  display: flex;
  gap: 0.25rem;
}
.error {
  color: #c0392b;
}
</style>
