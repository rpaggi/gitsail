// The typed frontend service for the recent-repositories list (US-052),
// following `repository.ts`'s convention: this is the only module that
// calls `invoke("list_recent_repositories" | "forget_recent_repository", ...)`
// — components/stores never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type { RecentRepositoryDto } from "./dto";

export async function listRecentRepositories(): Promise<RecentRepositoryDto[]> {
  return invoke<RecentRepositoryDto[]>("list_recent_repositories");
}

// Removing an entry is the explicit "not found — remove from list?"
// confirmation US-052 criterion 2 requires; nothing calls this
// automatically.
export async function forgetRecentRepository(path: string): Promise<RecentRepositoryDto[]> {
  return invoke<RecentRepositoryDto[]>("forget_recent_repository", { path });
}
