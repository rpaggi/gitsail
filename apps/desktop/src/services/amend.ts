// The typed frontend service for amending HEAD (US-059). `previewAmend`
// is read-only (never touches the index, working tree, or HEAD); its
// `head.hash` is exactly what must be echoed back as `amendCommit`'s
// `expectedHead` — the Core revalidates it is still `HEAD` immediately
// before amending and refuses otherwise (US-059 criterion 3), so a stale
// preview can never rewrite the wrong commit.

import { invoke } from "@tauri-apps/api/core";

import type { AmendPreviewDto, CommitResultDto } from "./dto";

export async function previewAmend(): Promise<AmendPreviewDto> {
  return invoke<AmendPreviewDto>("preview_amend");
}

export async function amendCommit(message: string, expectedHead: string): Promise<CommitResultDto> {
  return invoke<CommitResultDto>("amend_commit", { message, expectedHead });
}
