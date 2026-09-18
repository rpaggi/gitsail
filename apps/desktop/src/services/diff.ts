// The typed frontend service for reading a diff (US-057). Both the
// unified and side-by-side presentations are derived, in the frontend,
// from this single `DiffDto` — there is exactly one read per
// file/side/base-target combination, never one per view mode.

import { invoke } from "@tauri-apps/api/core";

import type { DiffDto } from "./dto";

export interface GetDiffParams {
  staged: boolean;
  path?: string;
}

export async function getDiff(params: GetDiffParams): Promise<DiffDto> {
  return invoke<DiffDto>("get_diff", {
    staged: params.staged,
    path: params.path ?? null,
  });
}
