<script setup lang="ts">
// Remote sync: fetch/pull/push (US-060/T-193). Mirrors `gitsail-tui`'s own
// T-182 remote-sync policy exactly (same `resolve_sync_target` resolution
// rule, fast-forward-only pull, plain non-force push), so the Desktop and
// TUI reach the same Git end-state for the same scenario (this story's
// DoD). Every mutation goes through `stores/operation.ts`'s shared confirm/
// run/result flow — this component never calls `services/sync.ts` directly.

import { onMounted } from "vue";

import { useSyncStore } from "../stores/sync";

const sync = useSyncStore();

onMounted(() => {
  void sync.loadRemotes();
  void sync.refreshTarget();
});
</script>

<template>
  <div class="sync-panel">
    <h3>Sync</h3>

    <p v-if="sync.resolveError" class="error">{{ sync.resolveError.message }}</p>
    <p v-else-if="sync.resolvedTarget" class="sync-panel__target">
      Target:
      <template v-if="sync.resolvedTarget.branch">
        branch '<strong>{{ sync.resolvedTarget.branch }}</strong>' &lt;-&gt; remote '<strong>{{
          sync.resolvedTarget.remote
        }}</strong>'
      </template>
      <template v-else>
        remote '<strong>{{ sync.resolvedTarget.remote }}</strong>'
      </template>
    </p>

    <ul v-if="sync.remotes.length > 0" class="sync-panel__remotes">
      <li v-for="remote in sync.remotes" :key="remote.name">
        {{ remote.name }} — {{ remote.fetchUrl }}
      </li>
    </ul>

    <div class="sync-panel__actions">
      <button :disabled="sync.resolveError !== null" @click="sync.requestFetch()">Fetch</button>
      <button :disabled="sync.resolveError !== null" @click="sync.requestPull()">Pull</button>
      <button :disabled="sync.resolveError !== null" @click="sync.requestPush()">Push</button>
    </div>

    <p v-if="sync.lastFetchResult" class="sync-panel__result">
      Fetched from '{{ sync.lastFetchResult.remote }}'.
    </p>
    <p v-if="sync.lastPullResult" class="sync-panel__result">
      Pull '{{ sync.lastPullResult.branch }}' from '{{ sync.lastPullResult.remote }}':
      <template v-if="sync.lastPullResult.outcome.outcome === 'fastForwarded'">
        fast-forwarded to {{ sync.lastPullResult.outcome.newHead.slice(0, 8) }}
      </template>
      <template v-else>already up to date</template>
    </p>
    <p v-if="sync.lastPushResult" class="sync-panel__result">
      Pushed '{{ sync.lastPushResult.branch }}' to '{{ sync.lastPushResult.remote }}'.
    </p>
  </div>
</template>

<style scoped>
.sync-panel {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}
.sync-panel__remotes {
  list-style: none;
  margin: 0;
  padding: 0;
  opacity: 0.8;
  font-size: 0.85rem;
}
.sync-panel__actions {
  display: flex;
  gap: 0.5rem;
}
.sync-panel__result {
  margin: 0;
  opacity: 0.85;
}
.error {
  color: #c0392b;
}
</style>
