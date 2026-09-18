// The typed frontend service for the startup handoff (closing the EPIC-15
// gap: VS Code's T-210/US-077 launches this process with `--repo <path>
// --commit <hash>`, which the Desktop process previously ignored
// entirely). Consumed exactly once — the backend's `take_startup_intent`
// clears it after this call, so calling it twice (e.g. a component
// re-mount) returns an empty intent the second time rather than
// re-opening/re-selecting the same target again.

import { invoke } from "@tauri-apps/api/core";

import type { StartupIntentDto } from "./dto";

export async function takeStartupIntent(): Promise<StartupIntentDto> {
  return invoke<StartupIntentDto>("take_startup_intent");
}
