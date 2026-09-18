// The typed frontend service for the commit graph (US-067), following
// `repository.ts`'s convention: this is the only module that calls
// `invoke("get_commit_graph_page", ...)` — components/stores never call
// `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type { CommitGraphPageDto } from "./dto";

export interface CommitGraphPageParams {
  // Scopes history to this branch (mirrors `CommitQuery.branch`); omit for
  // the default (`HEAD`).
  branch?: string;
  // The previous page's `nextCursor`; omit for the first page.
  cursor?: string;
  // Page size; omit to use the backend's own default.
  limit?: number;
  // `true` starts a brand new commit graph on the backend before laying
  // out this page (e.g. the branch filter changed); `false` continues
  // appending to whatever has already been accumulated.
  reset: boolean;
}

export async function getCommitGraphPage(
  params: CommitGraphPageParams,
): Promise<CommitGraphPageDto> {
  return invoke<CommitGraphPageDto>("get_commit_graph_page", {
    branch: params.branch ?? null,
    cursor: params.cursor ?? null,
    limit: params.limit ?? null,
    reset: params.reset,
  });
}
