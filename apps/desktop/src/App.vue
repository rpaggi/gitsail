<script setup lang="ts">
import CommitGraph from "./components/CommitGraph.vue";
import RepositoryOpener from "./components/RepositoryOpener.vue";
import StatusPanel from "./components/StatusPanel.vue";
</script>

<template>
  <main>
    <h1>GitSail</h1>
    <RepositoryOpener />
    <StatusPanel />
    <section class="graph-section">
      <CommitGraph />
    </section>
  </main>
</template>

<style>
body {
  margin: 0;
  font-family: system-ui, sans-serif;
  background: #1e1e1e;
  color: #e0e0e0;
}
main {
  padding: 1.5rem;
}
.graph-section {
  height: 60vh;
  margin-top: 1rem;
  border: 1px solid #444;
}
</style>
