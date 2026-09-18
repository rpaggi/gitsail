// Pure presentation helpers for `PullRequestsPanel.vue` (T-245/US-103),
// extracted and unit-tested directly rather than through the component —
// same convention `diffPresentation.ts`/`commitGraphLayout.ts` already
// establish in this directory.

import type { PullRequestStateDto } from "../services/dto";

/** The human-readable label for a PR/MR's state (US-103 criterion 1). */
export function pullRequestStateLabel(state: PullRequestStateDto): string {
  switch (state) {
    case "open":
      return "Open";
    case "merged":
      return "Merged";
    case "closed":
      return "Closed";
  }
}

/**
 * The "source -> target" branch summary line, or `null` when either branch
 * is missing (US-103 criterion 1: "branches quando disponíveis" — a
 * missing branch, e.g. one since deleted, is simply omitted rather than
 * rendered as a blank/placeholder value).
 */
export function pullRequestBranchSummary(
  sourceBranch: string | null,
  targetBranch: string | null,
): string | null {
  if (sourceBranch === null || targetBranch === null) {
    return null;
  }
  return `${sourceBranch} → ${targetBranch}`;
}

/** The author label, distinguishing a genuinely unknown author (US-103
 * criterion 1: "autor... quando disponível") from any real name — never an
 * empty string, which could otherwise look like a rendering bug rather
 * than "the forge reported no author". */
export function pullRequestAuthorLabel(author: string | null): string {
  return author ?? "(unknown)";
}
