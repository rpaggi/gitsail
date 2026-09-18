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

export async function getRepositoryStatus(): Promise<RepositoryStatusDto> {
  return invoke<RepositoryStatusDto>("get_repository_status");
}
