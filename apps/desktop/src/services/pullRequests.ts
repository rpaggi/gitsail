// The typed frontend service for PR/MR listing (EPIC-20; T-245/US-103).
// Following `forge.ts`'s own convention: this is the only module that calls
// `invoke("list_pull_requests" | "open_pull_request_link", ...)`. No forge
// API logic lives here — every call is a thin pass-through to a Tauri
// command that itself only calls `gitsail-application`/`gitsail-forge`
// (AGENTS.md).

import { invoke } from "@tauri-apps/api/core";

import type { ListPullRequestsOutcomeDto } from "./dto";

/**
 * Lists one page of PRs/MRs for the current repository's detected forge
 * remote (US-103 criterion 1). `page` is 1-based.
 *
 * Never a rejected promise for a forge-API-level failure (no token,
 * insufficient permission, rate limiting, offline/network failure): each of
 * those is its own tagged `ListPullRequestsOutcomeDto` variant (US-103
 * criterion 2), so the caller never needs a `catch` to distinguish "no
 * PRs/MRs" from "could not check". A rejected promise here only ever means
 * no repository is open yet.
 */
export async function listPullRequests(page: number): Promise<ListPullRequestsOutcomeDto> {
  return invoke<ListPullRequestsOutcomeDto>("list_pull_requests", { page });
}

/**
 * Opens a PR/MR's own web page in the browser (US-103 criterion 3: always
 * an explicit user action — call this only from a direct click handler,
 * never automatically/on load). Resolves to `false` (never a rejected
 * promise) when `url` does not pass the backend's own host/scheme
 * validation against the currently detected forge remote.
 */
export async function openPullRequestLink(url: string): Promise<boolean> {
  return invoke<boolean>("open_pull_request_link", { url });
}
