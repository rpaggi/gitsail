<script setup lang="ts">
// Pull/Merge Request listing, in explicitly limited scope (T-245/US-103):
// title/state/author/branches only — no diffs, comments, or CI status (see
// `gitsail_application::pull_requests`'s module doc for the full scope
// cut). Every state US-103 criterion 2 requires is rendered as its own,
// visually distinct block below (never inferred from an empty list), and
// "open in browser" is always a per-item, explicit click (criterion 3) —
// never triggered on mount/hover/render.
//
// Untrusted content (title/author/branch names, all forge/repository-
// authored): rendered exclusively through Vue's `{{ }}` text
// interpolation, which HTML-escapes by default — this template never uses
// `v-html` anywhere, the same discipline the VS Code extension's
// `hoverSanitizer` module documents for commit hover Markdown.

import { onMounted } from "vue";

import { pullRequestAuthorLabel, pullRequestBranchSummary, pullRequestStateLabel } from "./pullRequestsPresentation";
import { usePullRequestsStore } from "../stores/pullRequests";

const pullRequests = usePullRequestsStore();

onMounted(() => {
  void pullRequests.loadFirstPage();
});
</script>

<template>
  <div class="pull-requests-panel">
    <h3>Pull/Merge Requests</h3>

    <p v-if="pullRequests.status === 'idle' || pullRequests.status === 'loading'" class="pull-requests-panel__status">
      Loading…
    </p>

    <p v-else-if="pullRequests.status === 'noForge'" class="pull-requests-panel__status">
      No GitHub/GitLab remote detected for this repository.
    </p>

    <div v-else-if="pullRequests.status === 'noToken'" class="pull-requests-panel__status pull-requests-panel__status--warning">
      <p>
        No PRs/MRs could be listed: either no account is connected, or the
        connected token does not have access to this repository (it may be
        private).
      </p>
      <button @click="pullRequests.loadFirstPage()">Retry</button>
    </div>

    <div v-else-if="pullRequests.status === 'rateLimited'" class="pull-requests-panel__status pull-requests-panel__status--warning">
      <p v-if="pullRequests.retryAfterSeconds !== null">
        Rate limited by the forge API — try again in {{ pullRequests.retryAfterSeconds }}s.
      </p>
      <p v-else>Rate limited by the forge API — try again later.</p>
      <button @click="pullRequests.loadFirstPage()">Retry</button>
    </div>

    <div v-else-if="pullRequests.status === 'offline'" class="pull-requests-panel__status pull-requests-panel__status--error">
      <p>Could not reach the forge API: {{ pullRequests.offlineMessage }}</p>
      <button @click="pullRequests.loadFirstPage()">Retry</button>
    </div>

    <div v-else-if="pullRequests.status === 'error'" class="pull-requests-panel__status pull-requests-panel__status--error">
      <p>{{ pullRequests.errorMessage }}</p>
      <button @click="pullRequests.loadFirstPage()">Retry</button>
    </div>

    <template v-else-if="pullRequests.status === 'loaded'">
      <p v-if="pullRequests.page && pullRequests.page.items.length === 0" class="pull-requests-panel__status">
        No pull/merge requests found.
      </p>

      <ul v-else-if="pullRequests.page" class="pull-requests-panel__list">
        <li v-for="(item, index) in pullRequests.page.items" :key="index" class="pull-requests-panel__item">
          <div class="pull-requests-panel__item-header">
            <span class="pull-requests-panel__badge" :class="`pull-requests-panel__badge--${item.state}`">
              {{ pullRequestStateLabel(item.state) }}
            </span>
            <span class="pull-requests-panel__title">{{ item.title }}</span>
          </div>
          <div class="pull-requests-panel__meta">
            <span>by {{ pullRequestAuthorLabel(item.author) }}</span>
            <span v-if="pullRequestBranchSummary(item.sourceBranch, item.targetBranch)">
              — {{ pullRequestBranchSummary(item.sourceBranch, item.targetBranch) }}
            </span>
          </div>
          <button :aria-label="`Open ${item.title} in browser`" @click="pullRequests.openInBrowser(item.url)">Open in browser</button>
        </li>
      </ul>

      <div class="pull-requests-panel__pagination">
        <button :disabled="pullRequests.currentPage <= 1" @click="pullRequests.previousPage()">Previous</button>
        <span>Page {{ pullRequests.currentPage }}</span>
        <button :disabled="!pullRequests.page?.hasNextPage" @click="pullRequests.nextPage()">Next</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.pull-requests-panel {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.pull-requests-panel__status {
  margin: 0;
  opacity: 0.85;
}
.pull-requests-panel__status--warning {
  color: #d4a017;
}
.pull-requests-panel__status--error {
  color: #c0392b;
}
.pull-requests-panel__list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.pull-requests-panel__item {
  border: 1px solid #444;
  border-radius: 4px;
  padding: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.pull-requests-panel__item-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.pull-requests-panel__badge {
  font-size: 0.75rem;
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  text-transform: uppercase;
}
.pull-requests-panel__badge--open {
  background: #2e7d32;
}
.pull-requests-panel__badge--merged {
  background: #6a1b9a;
}
.pull-requests-panel__badge--closed {
  background: #757575;
}
.pull-requests-panel__title {
  font-weight: 600;
}
.pull-requests-panel__meta {
  font-size: 0.85rem;
  opacity: 0.8;
  display: flex;
  gap: 0.4rem;
}
.pull-requests-panel__pagination {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
