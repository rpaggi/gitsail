// The typed frontend service for local branch operations (US-060's
// create/switch/delete subset; remote sync — fetch/pull/push — is a
// separate concern in `services/sync.ts`/`stores/sync.ts`). Following
// `repository.ts`'s convention: this is the only module that calls
// `invoke("list_branches" | "create_branch" | "switch_branch" |
// "delete_branch", ...)`.

import { invoke } from "@tauri-apps/api/core";

import type { BranchDto } from "./dto";

export async function listBranches(): Promise<BranchDto[]> {
  return invoke<BranchDto[]>("list_branches");
}

/**
 * Creates a local branch named `name`. `startPoint` is any revision
 * expression the Core can resolve (a branch name, a commit hash, `HEAD`,
 * ...); omit it to start the branch at the current `HEAD` (US-060/T-193's
 * "reuses the same use cases as the TUI" criterion — the Core resolves the
 * start point, this never guesses at a hash itself).
 */
export async function createBranch(name: string, startPoint?: string): Promise<void> {
  return invoke<void>("create_branch", { name, startPoint: startPoint ?? null });
}

export async function switchBranch(target: string): Promise<void> {
  return invoke<void>("switch_branch", { target });
}

/**
 * Deletes the local branch `name`. `force: false` (the default a caller
 * should offer first) refuses to delete a branch with unmerged commits;
 * `force: true` is the reinforced-confirmation path (T-194) for deleting it
 * anyway.
 */
export async function deleteBranch(name: string, force: boolean): Promise<void> {
  return invoke<void>("delete_branch", { name, force });
}

/**
 * Renames the local branch `oldName` to `newName` (T-157/US-024). Never
 * overwrites a colliding `newName` — the Core refuses that outright (never
 * force) — and preserves any upstream `oldName` had configured; refreshing
 * the branch list after this resolves is the caller's job (mirrors
 * `switchBranch`/`deleteBranch`'s own contract).
 */
export async function renameBranch(oldName: string, newName: string): Promise<void> {
  return invoke<void>("rename_branch", { oldName, newName });
}
