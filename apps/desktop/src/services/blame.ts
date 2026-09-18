// The typed frontend service for reading file blame (EPIC-07/US-031..034,
// exposed to Desktop by T-195/US-062). Following `diff.ts`'s convention:
// this is the only module that calls `invoke("get_blame", ...)`. No Git
// logic lives here — this is a thin pass-through to a Tauri command that
// itself only calls `gitsail-application::GetFileBlame` (AGENTS.md).

import { invoke } from "@tauri-apps/api/core";

import type { BlameDto } from "./dto";

export interface GetBlameParams {
  path: string;
  /** Omit to blame the working tree, including any uncommitted changes
   * (US-033) — the same "missing means working tree" convention
   * `services/diff.ts`'s `GetDiffParams` already uses. */
  revision?: string;
}

export async function getBlame(params: GetBlameParams): Promise<BlameDto> {
  return invoke<BlameDto>("get_blame", {
    path: params.path,
    revision: params.revision ?? null,
  });
}
