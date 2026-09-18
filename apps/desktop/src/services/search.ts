// The typed frontend service for commit search and single-commit lookup
// (US-056). `searchCommits` reuses exactly the Core's `CommitQuery` filter
// set (hash/message via `textQuery`, `author`, `branch`,
// `revisionRange`) — the same filters `gitsail-tui`'s T-178 search already
// exercises — rather than inventing a second query shape. There is
// deliberately no `tag` filter here: the Core read port has no "list every
// tag" capability yet (EPIC-18, not yet built), so tag search is out of
// scope; a tag decoration on an already-loaded commit is still visible via
// `CommitDto.decorations`.

import { invoke } from "@tauri-apps/api/core";

import type { CommitDto } from "./dto";

export interface SearchCommitsParams {
  textQuery?: string;
  author?: string;
  branch?: string;
  revisionRange?: string;
  limit?: number;
}

export async function searchCommits(params: SearchCommitsParams = {}): Promise<CommitDto[]> {
  return invoke<CommitDto[]>("search_commits", {
    textQuery: params.textQuery ?? null,
    author: params.author ?? null,
    branch: params.branch ?? null,
    revisionRange: params.revisionRange ?? null,
    limit: params.limit ?? null,
  });
}

/** A plausible full or abbreviated commit hash (7-40 hex characters) —
 * used to decide whether a search box's text is worth trying as an exact
 * `getCommit` lookup in addition to a text-query search (US-056 criterion
 * 1: "busca aceita hash"). */
export function looksLikeCommitHash(text: string): boolean {
  return /^[0-9a-f]{7,40}$/i.test(text.trim());
}

export async function getCommit(hash: string): Promise<CommitDto> {
  return invoke<CommitDto>("get_commit", { hash });
}
