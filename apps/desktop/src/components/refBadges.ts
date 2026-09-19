// Turns a commit's `DecorationDto[]` into the small colored ref chips the
// commit list and commit-details card render ("main", "feature/ui", "HEAD",
// "v1.0.0", "origin/main").
//
// Pure, so the ordering and the remote-filtering rule below are unit
// tested rather than asserted by eye against a screenshot.

import type { DecorationDto } from "../services/dto";

export type RefBadgeKind = "head" | "branch" | "remote" | "tag";

export interface RefBadge {
  kind: RefBadgeKind;
  /** What the chip shows. Remote branches keep their `remote/branch`
   * qualifier, because "main" and "origin/main" are genuinely different
   * refs that routinely point at different commits — collapsing them
   * would hide exactly the divergence a history view exists to reveal. */
  label: string;
}

/** Sort weight per kind. HEAD first (it answers "where am I?", the single
 * most-scanned fact in a history list), then local branches, then tags,
 * then remote-tracking refs — which are the most numerous and least
 * actionable, so they never push the branch you are on out of view when a
 * commit carries a dozen decorations. */
const KIND_ORDER: Record<RefBadgeKind, number> = {
  head: 0,
  branch: 1,
  tag: 2,
  remote: 3,
};

export interface RefBadgeOptions {
  /** When false, remote-tracking refs are dropped entirely. This is what
   * the commit list's "Show remote branches" toggle drives: on a repo with
   * several remotes every commit otherwise carries a row of near-duplicate
   * chips that crowds out the local branch names. */
  includeRemote: boolean;
}

export function refBadges(
  decorations: DecorationDto[],
  options: RefBadgeOptions,
): RefBadge[] {
  const badges: RefBadge[] = [];
  for (const decoration of decorations) {
    switch (decoration.kind) {
      case "head":
        badges.push({ kind: "head", label: "HEAD" });
        break;
      case "branch":
        badges.push({ kind: "branch", label: decoration.name });
        break;
      case "tag":
        badges.push({ kind: "tag", label: decoration.name });
        break;
      case "remoteBranch":
        if (options.includeRemote) {
          badges.push({
            kind: "remote",
            label: `${decoration.remote}/${decoration.branch}`,
          });
        }
        break;
    }
  }
  // A stable sort by kind only — within one kind the Core's own ordering is
  // preserved, so chips never reshuffle between renders of the same commit.
  return badges.sort((a, b) => KIND_ORDER[a.kind] - KIND_ORDER[b.kind]);
}
