// Pure blame-line presentation helpers (T-195/US-062 criterion 1),
// extracted from `BlamePanel.vue` for direct unit testing — mirrors this
// project's `diffPresentation.ts`/`commitGraphLayout.ts` convention.
//
// `isUncommittedLine`/`ZERO_COMMIT_HASH` mirror `apps/vscode/src/
// blameFormat.ts`'s own handling exactly (US-033 criterion 1: "linhas
// alteradas localmente têm estado próprio" — never attributed to a real
// author or opened as a real commit).

import type { BlameLineDto } from "../services/dto";

/** Fixed sentinel commit hash Git uses for "not committed yet" blame lines
 * (all-zero SHA-1) — matches `gitsail_domain::CommitHash::is_zero()`. */
export const ZERO_COMMIT_HASH = "0".repeat(40);

/** Whether `line` reflects uncommitted working-tree content rather than a
 * real, historical commit (US-033 criterion 1) — checked via both the
 * domain's own `origin` field and the zero-hash sentinel, matching
 * `apps/vscode/src/blameFormat.ts::describeBlameLine`'s own belt-and-braces
 * check. A caller must never treat such a line as openable via
 * `stores/blame.ts`'s `openCommitDetails` — there is no real commit to
 * open. */
export function isUncommittedLine(line: BlameLineDto): boolean {
  return line.origin === "local" || line.commit === ZERO_COMMIT_HASH;
}

/** Abbreviates a full hash to its short form (8 characters), matching
 * `apps/vscode/src/blameFormat.ts::abbreviateHash`'s own default length —
 * the same abbreviation length `gitsail-tui`'s `CommitHash::to_short(8)`
 * renders in its own References/graph panels. */
export function abbreviateHash(hash: string, length = 8): string {
  return hash.slice(0, length);
}
