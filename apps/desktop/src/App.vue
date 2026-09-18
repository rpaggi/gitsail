<script setup lang="ts">
import RepositoryOpener from "./components/RepositoryOpener.vue";
import StatusPanel from "./components/StatusPanel.vue";
</script>

<template>
  <main>
    <h1>GitSail</h1>
    <RepositoryOpener />
    <StatusPanel />
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
</style>
