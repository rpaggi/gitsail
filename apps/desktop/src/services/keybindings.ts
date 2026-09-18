// The typed frontend service for custom keyboard shortcut overrides
// (T-249/US-107), following `preferences.ts`'s convention: this is the
// only module that calls `invoke("get_keybinding_overrides" | ...)`.
//
// Every function here trades in a plain `action id -> binding string` map
// (`Record<string, string>`) — the backend
// (`keybindings_store::JsonFileKeybindingsStore`) has no notion of the
// action registry itself; that lives in `../keybindings.ts`, which merges
// these overrides with each action's default binding.

import { invoke } from "@tauri-apps/api/core";

export async function getKeybindingOverrides(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_keybinding_overrides");
}

export async function setKeybindingOverride(
  actionId: string,
  binding: string,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("set_keybinding_override", { actionId, binding });
}

/** Clears one action's override, reverting it to its default binding
 * (US-107 criterion 1). */
export async function resetKeybindingOverride(actionId: string): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("reset_keybinding_override", { actionId });
}

/** Clears every override at once. */
export async function resetAllKeybindingOverrides(): Promise<void> {
  return invoke<void>("reset_all_keybinding_overrides");
}
