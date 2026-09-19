<script setup lang="ts">
// Keyboard shortcuts settings panel (T-249/US-107): lists every
// configurable action with its current binding, a "Remap" capture flow,
// a per-action "Reset" (restore default — criterion 1), an "all" reset,
// and a conflict warning per action sharing a binding with another
// (criterion 2). `SearchPalette.vue` reads the same store's `bindings` to
// show the currently-bound shortcut for its own action (criterion 3).

import { onBeforeUnmount, ref, watch } from "vue";

import { useKeybindingsStore } from "../stores/keybindings";
import { CONFIGURABLE_ACTIONS, bindingFromKeyboardEvent, formatBindingForDisplay } from "../keybindings";

const keybindings = useKeybindingsStore();

const remappingActionId = ref<string | null>(null);
const remapHint = ref<string | null>(null);

function startRemap(actionId: string): void {
  remappingActionId.value = actionId;
  remapHint.value = "Press a shortcut (must include Ctrl or Cmd)… Esc to cancel.";
}

function cancelRemap(): void {
  remappingActionId.value = null;
  remapHint.value = null;
}

function onCaptureKeydown(event: KeyboardEvent): void {
  const actionId = remappingActionId.value;
  if (actionId === null) {
    return;
  }
  event.preventDefault();
  event.stopPropagation();
  if (event.key === "Escape") {
    cancelRemap();
    return;
  }
  const binding = bindingFromKeyboardEvent(event);
  if (binding === null) {
    remapHint.value = "That key needs Ctrl or Cmd held — try again, or Esc to cancel.";
    return;
  }
  cancelRemap();
  void keybindings.setBinding(actionId, binding);
}

// The capture-phase listener only exists while an actual remap is in
// progress, and it stops propagation on every key it consumes — so it can
// never be mistaken by the global shortcut dispatcher (`AppShell.vue`) for
// an ordinary shortcut press.
watch(remappingActionId, (value, previous) => {
  if (previous !== null) {
    window.removeEventListener("keydown", onCaptureKeydown, true);
  }
  if (value !== null) {
    window.addEventListener("keydown", onCaptureKeydown, true);
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onCaptureKeydown, true);
});

function conflictPartnerLabels(actionId: string): string[] {
  const binding = keybindings.bindings[actionId];
  const group = keybindings.conflicts.get(binding);
  if (!group) {
    return [];
  }
  return group
    .filter((id) => id !== actionId)
    .map((id) => CONFIGURABLE_ACTIONS.find((action) => action.id === id)?.label ?? id);
}
</script>

<template>
  <div class="keybindings-panel">
    <div class="keybindings-panel__header">
      <button type="button" @click="keybindings.resetAll()">Restore all defaults</button>
    </div>

    <ul class="keybindings-panel__list">
      <li v-for="action in CONFIGURABLE_ACTIONS" :key="action.id" class="keybindings-panel__row">
        <span class="keybindings-panel__label">{{ action.label }}</span>
        <code class="keybindings-panel__binding">{{ formatBindingForDisplay(keybindings.bindings[action.id]) }}</code>
        <button type="button" @click="startRemap(action.id)">
          {{ remappingActionId === action.id ? "Press a key…" : "Remap" }}
        </button>
        <button
          type="button"
          :disabled="keybindings.bindings[action.id] === action.defaultBinding"
          @click="keybindings.resetBinding(action.id)"
        >
          Reset
        </button>
        <p
          v-if="conflictPartnerLabels(action.id).length > 0"
          class="keybindings-panel__conflict"
          role="alert"
        >
          Also bound to {{ conflictPartnerLabels(action.id).join(", ") }}
        </p>
      </li>
    </ul>

    <p v-if="remapHint" class="keybindings-panel__hint" role="status">{{ remapHint }}</p>
    <p v-if="keybindings.lastError" class="keybindings-panel__error" role="alert">
      {{ keybindings.lastError.message }}
    </p>
  </div>
</template>

<style scoped>
.keybindings-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
}
.keybindings-panel__list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}
.keybindings-panel__row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.keybindings-panel__label {
  flex: 1;
  min-width: 7rem;
}
.keybindings-panel__binding {
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0.1rem 0.4rem;
  font-family: var(--font-mono);
}
.keybindings-panel__conflict {
  flex-basis: 100%;
  margin: 0;
  color: var(--color-warning);
  font-size: 0.8rem;
}
.keybindings-panel__hint {
  margin: 0.5rem 0 0;
  color: var(--color-text-muted);
  font-size: 0.8rem;
}
.keybindings-panel__error {
  margin: 0.5rem 0 0;
  color: var(--color-danger);
  font-size: 0.8rem;
}
</style>
