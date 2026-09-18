// The typed frontend service for remote sync (US-060/T-193): fetch/pull/push
// plus resolving which remote/branch they would target. Following
// `branches.ts`'s convention: this is the only module that calls
// `invoke("list_remotes" | "resolve_sync_target" | "fetch" | "pull" |
// "push", ...)`. No Git logic lives here — every call is a thin pass-through
// to a Tauri command that itself only calls `gitsail-application` (AGENTS.md).

import { invoke } from "@tauri-apps/api/core";

import type { PullResultDto, RemoteDto, SyncTargetDto } from "./dto";

export async function listRemotes(): Promise<RemoteDto[]> {
  return invoke<RemoteDto[]>("list_remotes");
}

/**
 * Resolves which remote (and, for pull/push, which branch) a sync action
 * would target — without mutating anything (US-060 criterion 2: show the
 * remote/branch/upstream that would be affected *before* executing).
 * Mirrors `gitsail_tui::App::resolve_sync_remote`'s policy exactly: the
 * current branch's configured upstream when it has one, else the sole
 * configured remote, else an explicit "can't determine which to use" error
 * rather than a silent guess.
 */
export async function resolveSyncTarget(): Promise<SyncTargetDto> {
  return invoke<SyncTargetDto>("resolve_sync_target");
}

/** Fetches the resolved remote's refs (US-096). Never touches the working
 * tree/HEAD. */
export async function fetch(): Promise<SyncTargetDto> {
  return invoke<SyncTargetDto>("fetch");
}

/** Integrates the resolved remote's tracked branch via a fast-forward-only
 * pull (US-097). A divergence is reported as an ordinary rejected promise
 * (the Core's `operation_conflict`), never merged/rebased automatically. */
export async function pull(): Promise<PullResultDto> {
  return invoke<PullResultDto>("pull");
}

/** Publishes the current branch to the resolved remote via a plain,
 * non-force push (US-098). A non-fast-forward rejection is never escalated
 * to a force push automatically. */
export async function push(): Promise<SyncTargetDto> {
  return invoke<SyncTargetDto>("push");
}
