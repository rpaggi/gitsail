// Custom keyboard shortcut overrides (T-249/US-107), following
// `preferences.ts`'s convention: a thin Pinia wrapper over a typed service
// (`../services/keybindings.ts`). The action registry and every pure
// binding computation (merging defaults with overrides, conflict
// detection) live in `../keybindings.ts`; this store only adds the
// Tauri-backed persistence and reactive state around it.

import { defineStore } from "pinia";

import {
  getKeybindingOverrides,
  resetAllKeybindingOverrides,
  resetKeybindingOverride,
  setKeybindingOverride,
} from "../services/keybindings";
import { CONFIGURABLE_ACTIONS, effectiveBindings, findBindingConflicts } from "../keybindings";
import { isErrorPayload, type ErrorPayload } from "../services/errors";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useKeybindingsStore = defineStore("keybindings", {
  state: () => ({
    overrides: {} as Record<string, string>,
    isLoading: false,
    lastError: null as ErrorPayload | null,
  }),
  getters: {
    /** Every configurable action's currently-effective binding (default,
     * unless overridden) — the single source both the settings panel and
     * the global shortcut dispatcher (`AppShell.vue`) read, so they can
     * never disagree about what a shortcut currently does. */
    bindings(state): Record<string, string> {
      return effectiveBindings(CONFIGURABLE_ACTIONS, state.overrides);
    },
    /** Action ids grouped by any binding two or more of them currently
     * share (US-107 criterion 2) — empty when there are no conflicts. */
    conflicts(): Map<string, string[]> {
      return findBindingConflicts(this.bindings);
    },
  },
  actions: {
    async load(): Promise<void> {
      this.isLoading = true;
      try {
        this.overrides = await getKeybindingOverrides();
        this.lastError = null;
      } catch (error) {
        this.overrides = {};
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
      }
    },

    /** Remaps `actionId` to `binding` (US-107 criterion 1). Never refuses a
     * binding that conflicts with another action's — see
     * `findBindingConflicts`'s own doc for why a conflict is a warning, not
     * a block. */
    async setBinding(actionId: string, binding: string): Promise<void> {
      try {
        this.overrides = await setKeybindingOverride(actionId, binding);
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      }
    },

    /** Restores `actionId` to its default binding (US-107 criterion 1). */
    async resetBinding(actionId: string): Promise<void> {
      try {
        this.overrides = await resetKeybindingOverride(actionId);
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      }
    },

    /** Restores every action to its default binding at once. */
    async resetAll(): Promise<void> {
      try {
        await resetAllKeybindingOverrides();
        this.overrides = {};
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      }
    },
  },
});
