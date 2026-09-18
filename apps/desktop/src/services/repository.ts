// The typed frontend service (SAD §17: "Vue 3 UI -> Typed frontend service
// -> Tauri Commands"). This is the only module that calls `invoke` — Vue
// components must go through the functions here, never `invoke` directly,
// so the command boundary stays thin and typed end to end (US-051
// criterion 1).

import { invoke } from "@tauri-apps/api/core";

import type { RepositoryDto, RepositoryStatusDto } from "./dto";

export async function openRepository(path: string): Promise<RepositoryDto> {
  return invoke<RepositoryDto>("open_repository", { path });
}

// Mirrors `gitsail_application::RefreshReason` (US-054 criterion 2): manual,
// focus, and after-mutation refreshes are all the same command/read path —
// this only tags *why* it ran, it never changes what is fetched.
export type RefreshReason = "manual" | "focus" | "after_mutation";

export async function getRepositoryStatus(
  reason: RefreshReason = "manual",
): Promise<RepositoryStatusDto> {
  return invoke<RepositoryStatusDto>("get_repository_status", { reason });
}
