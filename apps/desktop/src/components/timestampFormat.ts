// Pure `GitTimestampDto` formatting, shared by `BlamePanel.vue` and
// `ReferencesPanel.vue` (T-195/US-062) — kept DOM-free so it is unit
// tested directly, matching this project's convention of extracting
// presentation math out of components (`diffPresentation.ts`,
// `commitGraphLayout.ts`).
//
// Mirrors `apps/vscode/src/blameFormat.ts`'s own `formatGitTimestamp`
// exactly (same shift-by-offset-then-read-UTC-fields approach) so a commit
// renders the same date on Desktop and in the VS Code extension, both
// reading the *commit's own* recorded offset rather than the host
// machine's timezone.

import type { GitTimestampDto } from "../services/dto";

/** Formats a `GitTimestampDto` as `YYYY-MM-DD` in the commit's own recorded
 * offset — never the viewer's local timezone, so the same commit renders
 * identically regardless of where GitSail runs. */
export function formatGitTimestamp(timestamp: GitTimestampDto): string {
  const shiftedMs = (timestamp.secondsSinceEpoch + timestamp.utcOffsetMinutes * 60) * 1000;
  const date = new Date(shiftedMs);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
