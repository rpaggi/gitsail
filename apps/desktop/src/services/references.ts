// The typed frontend service for read-only tag/stash inspection (EPIC-18/
// US-091, exposed to Desktop by T-195/US-062). Following `branches.ts`'s/
// `sync.ts`'s convention: this is the only module that calls
// `invoke("list_tags" | "list_stash_entries", ...)`. No Git logic lives
// here — every call is a thin pass-through to a Tauri command that itself
// only calls `gitsail-application` (AGENTS.md). Remotes are already served
// by `services/sync.ts`'s own `listRemotes` — not duplicated here.

import { invoke } from "@tauri-apps/api/core";

import type { StashDto, TagDto } from "./dto";

/** Lists local tags, both lightweight and annotated (US-091 criterion 1).
 * An empty repository legitimately resolves to an empty array — never a
 * rejected promise. */
export async function listTags(): Promise<TagDto[]> {
  return invoke<TagDto[]>("list_tags");
}

/** Lists stash entries, newest (`stash@{0}`) first (US-091 criterion 2). An
 * empty stash legitimately resolves to an empty array — never a rejected
 * promise. */
export async function listStashEntries(): Promise<StashDto[]> {
  return invoke<StashDto[]>("list_stash_entries");
}
