<script setup lang="ts">
// Local-branch list and create/switch/delete actions (T-193's local-branch
// subset of US-060). Every mutation goes through `stores/branches.ts`'s
// `request*` actions, which in turn go through T-194's shared confirmation
// flow — this component never calls `services/branches.ts` directly.

import { onMounted, ref } from "vue";

import { useBranchesStore } from "../stores/branches";

const branches = useBranchesStore();
const newBranchName = ref("");

function createBranch(): void {
  const name = newBranchName.value.trim();
  if (name.length === 0) {
    return;
  }
  void branches.requestCreate(name);
  newBranchName.value = "";
}

onMounted(() => {
  void branches.load();
});
</script>

<template>
  <div class="branch-panel">
    <h3>Branches</h3>
    <p v-if="branches.lastError" class="error">{{ branches.lastError.message }}</p>
    <ul>
      <li v-for="branch in branches.branches" :key="branch.name" :class="{ current: branch.isCurrent }">
        <span>{{ branch.name }}</span>
        <span class="branch-panel__actions">
          <button v-if="!branch.isCurrent" @click="branches.requestSwitch(branch.name)">
            Switch
          </button>
          <button v-if="!branch.isCurrent" @click="branches.requestDelete(branch.name, false)">
            Delete
          </button>
        </span>
      </li>
    </ul>
    <div class="branch-panel__create">
      <input v-model="newBranchName" type="text" placeholder="new-branch-name" @keyup.enter="createBranch" />
      <button :disabled="newBranchName.trim().length === 0" @click="createBranch">Create</button>
    </div>
  </div>
</template>

<style scoped>
.branch-panel ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.branch-panel li {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.15rem 0;
}
.branch-panel li.current {
  font-weight: 600;
}
.branch-panel__actions {
  display: flex;
  gap: 0.25rem;
}
.branch-panel__create {
  margin-top: 0.5rem;
  display: flex;
  gap: 0.5rem;
}
.error {
  color: #c0392b;
}
</style>
