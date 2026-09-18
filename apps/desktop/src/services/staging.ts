// The typed frontend service for staging and committing (US-058).
// File-level (`stagePaths`/`unstagePaths`) and hunk-level
// (`stageHunks`/`unstageHunks`) staging are both exposed, matching US-058
// criterion 1's "por arquivo/hunk" — the frontend picks whichever
// granularity the person acted on, never reimplementing the hunk-apply
// logic itself (that stays in `gitsail-git`).

import { invoke } from "@tauri-apps/api/core";

import type { CommitResultDto, FileDiffDto } from "./dto";

export async function stagePaths(paths: string[]): Promise<void> {
  return invoke<void>("stage_paths", { paths });
}

export async function unstagePaths(paths: string[]): Promise<void> {
  return invoke<void>("unstage_paths", { paths });
}

/**
 * Stages only the hunks carried by `selection`, each element a
 * `FileDiffDto` trimmed to the hunks to stage — typically a subset of what
 * `getDiff({ staged: false })` last returned. Only ever echoes back exactly
 * what the Core produced; the frontend never fabricates or edits hunks.
 */
export async function stageHunks(selection: FileDiffDto[]): Promise<void> {
  return invoke<void>("stage_hunks", { selection });
}

export async function unstageHunks(selection: FileDiffDto[]): Promise<void> {
  return invoke<void>("unstage_hunks", { selection });
}

export async function createCommit(message: string): Promise<CommitResultDto> {
  return invoke<CommitResultDto>("create_commit", { message });
}
