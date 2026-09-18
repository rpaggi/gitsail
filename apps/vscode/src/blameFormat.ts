// Pure formatting/config-parsing helpers for inline blame decorations
// (T-205/US-072). Kept entirely free of any `vscode` import so it is
// unit-testable as plain data-in/data-out, matching this package's existing
// convention (`documentState.ts`, `repositoryContext.ts`) of keeping
// business logic decoupled from the editor host — only `extension.ts` (or a
// thin adapter it owns) ever turns this module's output into a real
// `vscode.DecorationOptions`.
//
// This module never reimplements a Git rule (US-072 depends on US-034, the
// already-delivered blame engine) — it only decides how to *display* the
// `BlameLineDto` the CLI already computed.

import type { BlameLineDto, GitTimestampDto } from "./dto";

export type BlameDisplayMode = "currentLine" | "allVisibleLines";

/** T-250/US-108 criterion 2: relative ("3 days ago") vs. absolute
 * (`YYYY-MM-DD`) date rendering for `${date}` and the hover's date line. */
export type BlameDateStyle = "relative" | "absolute";

export interface BlameDisplayConfig {
  enabled: boolean;
  /** A template string using `${author}`, `${authorEmail}`, `${date}`,
   * `${hash}`, `${shortHash}`, `${message}` placeholders (US-072 criterion
   * 1: "formato configurável"). */
  format: string;
  /** US-072 criterion 2: "linha atual" vs "todas as linhas visíveis". */
  mode: BlameDisplayMode;
  /** Inactivity delay, in milliseconds, before a decoration is (re)computed
   * after the cursor stops moving (US-072 criterion 2). */
  delayMs: number;
  /** T-250/US-108 criterion 2: how `${date}` (and the hover's date line) is
   * rendered. */
  dateStyle: BlameDateStyle;
}

export const DEFAULT_BLAME_FORMAT = "${author}, ${date} • ${shortHash} • ${message}";
export const DEFAULT_BLAME_DELAY_MS = 400;
export const DEFAULT_BLAME_MODE: BlameDisplayMode = "currentLine";
/** Absolute, not relative: matches T-205/US-072's originally shipped
 * behavior (`formatGitTimestamp` was, until T-250, the *only* way a date
 * was ever rendered) — opting into relative dates is a deliberate choice a
 * person makes via settings, never a silent behavior change for existing
 * users. */
export const DEFAULT_BLAME_DATE_STYLE: BlameDateStyle = "absolute";

/** Reads `gitsail.blame.*` settings into a typed, validated config —
 * matching `controller.ts`'s existing `ConfigurationLike.get(key,
 * defaultValue)` shape so no new host abstraction is needed just to read
 * settings. An unrecognized `mode`/`dateStyle` value (e.g. hand-edited
 * settings.json with a typo) falls back to the documented default rather
 * than throwing, consistent with VS Code configuration generally being
 * untrusted, free-form JSON — and specifically so a bad value can never
 * turn into a retry loop or a broken decoration: reading configuration is
 * synchronous and always succeeds with *some* valid `BlameDisplayConfig`. */
export function readBlameDisplayConfig(get: <T>(key: string, defaultValue: T) => T): BlameDisplayConfig {
  const enabled = get<boolean>("blame.enabled", true);
  const format = get<string>("blame.format", DEFAULT_BLAME_FORMAT);
  const rawMode = get<string>("blame.mode", DEFAULT_BLAME_MODE);
  const mode: BlameDisplayMode = rawMode === "allVisibleLines" ? "allVisibleLines" : "currentLine";
  const rawDelay = get<number>("blame.delayMs", DEFAULT_BLAME_DELAY_MS);
  const delayMs = Number.isFinite(rawDelay) && rawDelay >= 0 ? rawDelay : DEFAULT_BLAME_DELAY_MS;
  const rawDateStyle = get<string>("blame.dateStyle", DEFAULT_BLAME_DATE_STYLE);
  const dateStyle: BlameDateStyle = rawDateStyle === "relative" ? "relative" : "absolute";
  return {
    enabled,
    format: format.length > 0 ? format : DEFAULT_BLAME_FORMAT,
    mode,
    delayMs,
    dateStyle,
  };
}

/** Formats a `GitTimestampDto` as `YYYY-MM-DD` *in the commit's own
 * recorded offset* (not the host machine's timezone), so the same commit
 * renders identically regardless of where the extension runs — computed by
 * shifting the epoch seconds by the offset and reading UTC fields back off
 * the result, never `Date`'s local-timezone accessors (which would leak the
 * host's own zone instead of the commit's). */
export function formatGitTimestamp(timestamp: GitTimestampDto): string {
  const shiftedMs = (timestamp.secondsSinceEpoch + timestamp.utcOffsetMinutes * 60) * 1000;
  const date = new Date(shiftedMs);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** "3 days ago"-style rendering of the real-time gap between a commit's
 * instant and `nowSecondsSinceEpoch` (T-250/US-108 criterion 2). Unlike
 * {@link formatGitTimestamp}, the commit's own UTC offset is irrelevant
 * here: a real elapsed-time gap between two instants never depends on
 * either instant's *calendar* timezone. A commit that appears to be in the
 * future (clock skew between machines) is clamped to `0` seconds — "just
 * now" — rather than rendering a nonsensical negative duration. */
export function formatRelativeDate(secondsSinceEpoch: number, nowSecondsSinceEpoch: number): string {
  const diffSeconds = Math.max(0, Math.round(nowSecondsSinceEpoch - secondsSinceEpoch));

  const unit = (value: number, singular: string): string => `${value} ${singular}${value === 1 ? "" : "s"} ago`;

  const diffMinutes = Math.floor(diffSeconds / 60);
  if (diffMinutes < 1) {
    return "just now";
  }
  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 1) {
    return unit(diffMinutes, "minute");
  }
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 1) {
    return unit(diffHours, "hour");
  }
  const diffMonths = Math.floor(diffDays / 30);
  if (diffMonths < 1) {
    return unit(diffDays, "day");
  }
  const diffYears = Math.floor(diffDays / 365);
  if (diffYears < 1) {
    return unit(diffMonths, "month");
  }
  return unit(diffYears, "year");
}

/** Renders a commit's timestamp per `dateStyle` (T-250/US-108 criterion
 * 2). `nowSecondsSinceEpoch` defaults to the real wall clock but is always
 * overridable, so every caller in this module stays pure/testable without
 * mocking global `Date` (matching this file's own no-`vscode`-import,
 * plain-data convention). */
export function formatBlameDate(
  timestamp: GitTimestampDto,
  dateStyle: BlameDateStyle,
  nowSecondsSinceEpoch: number = Date.now() / 1000,
): string {
  return dateStyle === "relative"
    ? formatRelativeDate(timestamp.secondsSinceEpoch, nowSecondsSinceEpoch)
    : formatGitTimestamp(timestamp);
}

/** The first line of a possibly multi-line commit subject/body, so a
 * one-line decoration never breaks across lines. */
export function firstLine(text: string): string {
  const newlineIndex = text.indexOf("\n");
  return newlineIndex === -1 ? text : text.slice(0, newlineIndex);
}

export function abbreviateHash(hash: string, length = 8): string {
  return hash.slice(0, length);
}

/** Fixed sentinel commit hash Git uses for "not committed yet" blame lines
 * (all-zero SHA-1) — matches `gitsail_domain::CommitHash::is_zero()`'s own
 * check. Recognizing it here is display-only (choosing not to render a
 * hash for it); the Core, not this extension, already decided the line's
 * `origin` is `"local"` because of it. */
export const ZERO_COMMIT_HASH = "0".repeat(40);

/** Substitutes template placeholders with `line`'s own fields. Never
 * invents a value: every placeholder comes directly from the `BlameLineDto`
 * the CLI returned. `dateText` overrides `${date}`'s rendering (T-250/
 * US-108 criterion 2: relative vs. absolute) — it defaults to
 * {@link formatGitTimestamp}'s absolute rendering so existing callers that
 * do not care about the distinction keep their original behavior. */
export function renderBlameTemplate(
  template: string,
  line: BlameLineDto,
  dateText: string = formatGitTimestamp(line.timestamp),
): string {
  const replacements: Record<string, string> = {
    "${author}": line.author.name,
    "${authorEmail}": line.author.email,
    "${date}": dateText,
    "${hash}": line.commit,
    "${shortHash}": abbreviateHash(line.commit),
    "${message}": firstLine(line.content.length > 0 ? line.content : ""),
  };
  // `line.content` above is the blamed *source* line, not the commit
  // message — `describeBlameDecorationText` overrides `${message}` with the
  // real commit subject for committed lines; this raw substitution alone is
  // only used as a fallback and by tests exercising template substitution
  // directly.
  let result = template;
  for (const [token, value] of Object.entries(replacements)) {
    result = result.split(token).join(value);
  }
  return result;
}

/** The commit subject to show in `${message}` for a *committed* line. Kept
 * separate from `BlameLineDto` (which has no subject field at all — only
 * the blamed line's own content) so a caller must explicitly supply it
 * (from a `CommitDto` fetched separately, or omit it) rather than this
 * module silently substituting the wrong text. */
export interface BlameDecorationText {
  /** Text to show inline (US-072 criterion 1). */
  readonly contentText: string;
  /** Plain-text (not yet sanitized/rendered) lines for the hover — built up
   * separately from `contentText` since the hover can carry more detail
   * (T-206) and, for a dirty document, an extra disclaimer line
   * (criterion 3) that would be too noisy inline. */
  readonly hoverLines: readonly string[];
}

/** Decides what to show for one blame line, honoring the "never invent an
 * author" rule (US-072/US-075 criterion 3):
 * - `origin: "local"` (Git's own "not committed yet" sentinel, US-033) never
 *   goes through the configurable template — a historical author/date/hash
 *   template does not apply to a line with no commit yet, so this always
 *   renders a fixed, honest "Uncommitted change" label instead.
 * - `documentDirty: true` (the editor buffer itself has unsaved changes,
 *   `documentState.ts`) appends an explicit disclaimer to the hover, since
 *   `gitsail-cli` only ever read the last-saved-to-disk content — it is
 *   never silently presented as describing the in-editor buffer.
 */
export function describeBlameLine(
  line: BlameLineDto,
  config: BlameDisplayConfig,
  commitSubject: string | undefined,
  documentDirty: boolean,
  nowSecondsSinceEpoch: number = Date.now() / 1000,
): BlameDecorationText {
  if (line.origin === "local" || line.commit === ZERO_COMMIT_HASH) {
    const hoverLines = ["Uncommitted change — not yet part of any commit."];
    if (documentDirty) {
      hoverLines.push(
        "This file also has unsaved editor changes; GitSail only ever reflects what is saved on disk.",
      );
    }
    return { contentText: "Uncommitted change", hoverLines };
  }

  const dateText = formatBlameDate(line.timestamp, config.dateStyle, nowSecondsSinceEpoch);
  const template = commitSubject !== undefined ? config.format.split("${message}").join("${__subject__}") : config.format;
  let contentText = renderBlameTemplate(template, line, dateText);
  if (commitSubject !== undefined) {
    contentText = contentText.split("${__subject__}").join(firstLine(commitSubject));
  }

  const hoverLines = [
    `${line.author.name} <${line.author.email}>`,
    `${dateText} • ${line.commit}`,
  ];
  if (commitSubject !== undefined && commitSubject.length > 0) {
    hoverLines.push(firstLine(commitSubject));
  }
  if (documentDirty) {
    hoverLines.push(
      "This file has unsaved editor changes; GitSail only ever reflects what is saved on disk.",
    );
  }
  return { contentText, hoverLines };
}
