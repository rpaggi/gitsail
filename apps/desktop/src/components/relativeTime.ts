// Pure "2 hours ago" formatting for `GitTimestampDto`, used by the commit
// list, the activity timeline and the commit-details card.
//
// Kept DOM-free and with `nowSeconds` injected rather than reading
// `Date.now()` internally, so it is unit tested directly — matching this
// project's convention of extracting presentation math out of components
// (`diffPresentation.ts`, `commitGraphLayout.ts`, `timestampFormat.ts`).
//
// This complements, and does not replace, `timestampFormat.ts`'s absolute
// `YYYY-MM-DD`: a relative label answers "how fresh is this" at a glance,
// which is what a history list needs, but it is inherently imprecise, so
// every caller pairs it with the absolute date in a `title`/tooltip rather
// than dropping the exact timestamp from the UI entirely.

import type { GitTimestampDto } from "../services/dto";

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

function plural(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? "" : "s"}`;
}

/**
 * Formats how long ago `timestamp` was, relative to `nowSeconds` (both in
 * Unix seconds).
 *
 * Deliberately coarse — one unit, never "1 hour 3 minutes" — because this
 * is a list column where a stable, short width matters more than
 * precision. Anything under a minute reads "just now" rather than
 * "0 minutes ago".
 *
 * A timestamp in the *future* (a commit with a skewed clock, or a machine
 * whose own clock is behind — both real and not rare in shared repos) is
 * reported as "just now" rather than as a negative or "in 3 hours": the
 * commit exists, so it is not actually in the future from the user's point
 * of view, and rendering clock skew as if it were meaningful history would
 * be misleading.
 */
export function formatRelativeTime(
  timestamp: GitTimestampDto,
  nowSeconds: number,
): string {
  const elapsed = nowSeconds - timestamp.secondsSinceEpoch;
  if (elapsed < MINUTE) {
    return "just now";
  }
  if (elapsed < HOUR) {
    return `${plural(Math.floor(elapsed / MINUTE), "minute")} ago`;
  }
  if (elapsed < DAY) {
    return `${plural(Math.floor(elapsed / HOUR), "hour")} ago`;
  }
  if (elapsed < MONTH) {
    return `${plural(Math.floor(elapsed / DAY), "day")} ago`;
  }
  if (elapsed < YEAR) {
    return `${plural(Math.floor(elapsed / MONTH), "month")} ago`;
  }
  return `${plural(Math.floor(elapsed / YEAR), "year")} ago`;
}
