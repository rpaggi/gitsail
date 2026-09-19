// `git log`/`git show -s` record parsing into `CommitDto`.
//
// Pure: takes a string, returns DTOs. It never spawns anything (only
// `process.ts` may).
//
// The format below is byte-for-byte the one `crates/gitsail-git`'s
// `LOG_FORMAT` already uses, and this parser reproduces
// `parse_log_record`/`parse_raw_date`/`parse_decorations` field for field.
// That is deliberate, and it is the concrete shape of the drift risk
// ADR-025 accepts: `test/gitParity.test.ts` compares this module's output
// against the real `gitsail` CLI's, so a divergence fails a test rather
// than silently showing a user different authorship in VS Code than in the
// TUI.

import { GitParseError } from "./errors";
import { CommitDto, DecorationDto, GitTimestampDto, SignatureDto } from "../dto";

/** ASCII Unit Separator: field delimiter inside one record. Chosen because
 * it can never appear in commit metadata, unlike a comma or pipe — the
 * "separator that cannot occur in the data" requirement. */
export const FIELD_SEP = "";
/** ASCII Record Separator: terminates each record. */
export const RECORD_SEP = "";

/** One `FIELD_SEP`-delimited record per commit, `RECORD_SEP`-terminated, in
 * the field order `parseCommitRecord` expects: hash, short hash, parents,
 * author name/email, committer name/email, author date, commit date,
 * subject, body, decorations. Identical to `gitsail-git`'s `LOG_FORMAT`. */
export const LOG_FORMAT =
  "%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%cn%x1f%ce%x1f%ad%x1f%cd%x1f%s%x1f%b%x1f%D%x1e";

const LOG_FIELD_COUNT = 12;

/**
 * Splits `git log --pretty=format:LOG_FORMAT` output into commits.
 *
 * `git log --pretty=format:` inserts a newline after every record but the
 * last, so each record after the first carries a leading `\n` that must be
 * stripped — the same quirk `parse_log_records` documents in Rust.
 */
export function parseCommitRecords(raw: string): CommitDto[] {
  return raw
    .split(RECORD_SEP)
    .map((record) => (record.startsWith("\n") ? record.slice(1) : record))
    .filter((record) => record.length > 0)
    .map(parseCommitRecord);
}

export function parseCommitRecord(record: string): CommitDto {
  const fields = record.split(FIELD_SEP);
  if (fields.length !== LOG_FIELD_COUNT) {
    throw new GitParseError(
      `Malformed git log record: expected ${LOG_FIELD_COUNT} fields, got ${fields.length}.`,
    );
  }

  const hash = fields[0];
  if (!/^[0-9a-f]{40,64}$/.test(hash)) {
    throw new GitParseError("Malformed git log record: first field was not a commit hash.");
  }
  const parents = fields[2].split(" ").filter((p) => p.length > 0);

  return {
    hash,
    shortHash: fields[1],
    parents,
    author: signature(fields[3], fields[4]),
    committer: signature(fields[5], fields[6]),
    authorDate: parseRawDate(fields[7]),
    commitDate: parseRawDate(fields[8]),
    subject: fields[9],
    body: fields[10],
    decorations: parseDecorations(fields[11]),
    // Derived exactly as `gitsail_protocol::CommitDto`'s `From<&Commit>`
    // derives them (`Commit::is_merge`/`is_root`), never independently
    // reinterpreted.
    isMerge: parents.length > 1,
    isRoot: parents.length === 0,
  };
}

function signature(name: string, email: string): SignatureDto {
  return { name, email };
}

/** Parses a `--date=raw` timestamp: `"<epoch seconds> <+HHMM|-HHMM>"`. */
export function parseRawDate(field: string): GitTimestampDto {
  const parts = field.split(" ");
  if (parts.length !== 2) {
    throw new GitParseError("Malformed --date=raw timestamp.");
  }
  const secondsSinceEpoch = Number(parts[0]);
  if (!Number.isInteger(secondsSinceEpoch)) {
    throw new GitParseError("Timestamp seconds were not an integer.");
  }
  return { secondsSinceEpoch, utcOffsetMinutes: parseUtcOffset(parts[1]) };
}

/** Parses a `+HHMM`/`-HHMM` UTC offset into signed minutes. */
export function parseUtcOffset(offset: string): number {
  if (offset.length !== 5) {
    throw new GitParseError("Timestamp offset must be sHHMM.");
  }
  const sign = offset[0] === "+" ? 1 : offset[0] === "-" ? -1 : 0;
  if (sign === 0) {
    throw new GitParseError("Timestamp offset must start with + or -.");
  }
  const hours = Number(offset.slice(1, 3));
  const minutes = Number(offset.slice(3, 5));
  if (!Number.isInteger(hours) || !Number.isInteger(minutes)) {
    throw new GitParseError("Timestamp offset was not numeric.");
  }
  return sign * (hours * 60 + minutes);
}

/**
 * Parses a `%D` decoration string (e.g. `HEAD -> main, origin/main, tag:
 * v1.0`).
 *
 * Only the `origin/` remote prefix is special-cased — a decoration for any
 * other remote falls back to a plain branch. That is a known simplification
 * carried over verbatim from `gitsail-git`'s `parse_decorations`, kept
 * identical on purpose: matching the Core's imperfection exactly is what
 * keeps the two from disagreeing on screen.
 */
export function parseDecorations(field: string): DecorationDto[] {
  if (field.length === 0) {
    return [];
  }
  const decorations: DecorationDto[] = [];
  for (const rawPart of field.split(", ")) {
    const part = rawPart.trim();
    if (part.length === 0) {
      continue;
    }
    if (part.startsWith("HEAD -> ")) {
      decorations.push({ kind: "head" });
      decorations.push({ kind: "branch", name: part.slice("HEAD -> ".length) });
    } else if (part === "HEAD") {
      decorations.push({ kind: "head" });
    } else if (part.startsWith("tag: ")) {
      decorations.push({ kind: "tag", name: part.slice("tag: ".length) });
    } else if (part.startsWith("origin/")) {
      decorations.push({
        kind: "remoteBranch",
        remote: "origin",
        branch: part.slice("origin/".length),
      });
    } else {
      decorations.push({ kind: "branch", name: part });
    }
  }
  return decorations;
}
