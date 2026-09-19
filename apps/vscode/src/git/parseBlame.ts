// `git blame --porcelain` parsing into `BlameLineDto[]`. Pure.
//
// The porcelain format's one real trap, and the reason this is not a
// line-by-line `map`: a commit's header block (`author`, `author-mail`,
// `author-time`, `author-tz`, ...) is emitted **only the first time that
// commit is mentioned**. Every later line attributed to the same commit is
// just a bare `<hash> <origLine> <finalLine> [<count>]` header followed by
// the tab-prefixed content line. So the parser must remember each commit's
// metadata in a map, keyed by hash, and look it up for the repeat mentions
// — reading the metadata positionally would attribute most of a file's
// lines to whichever commit happened to be described last.
//
// `--line-porcelain` would repeat the block every time and remove the trap,
// but it re-emits the full author/committer block per *line*: on a large
// file that is several times the output for information already known. The
// map is a few lines of code; the bandwidth is not worth it.
//
// Mirrors `crates/gitsail-git/src/provider.rs`'s `parse_blame`.

import { GitParseError } from "./errors";
import { BlameLineDto, GitTimestampDto, SignatureDto } from "../dto";
import { parseUtcOffset } from "./parseCommit";

/** Git's sentinel hash for a line that is not committed yet (all zeros). */
const ZERO_HASH_PATTERN = /^0+$/;

interface CommitMeta {
  author: SignatureDto;
  timestamp: GitTimestampDto;
}

/**
 * Parses `git blame --porcelain` output.
 *
 * Note the `origin` field this produces: a line whose hash is all zeros is
 * `"local"` (Git's "not committed yet"), anything else is `"committed"` —
 * the same rule `gitsail_domain::CommitHash::is_zero()` drives in the core.
 * Getting this wrong would put a fabricated author on an uncommitted line,
 * which `blameFormat.ts` explicitly refuses to do.
 */
export function parseBlamePorcelain(raw: string): BlameLineDto[] {
  const metadata = new Map<string, CommitMeta>();
  const lines: BlameLineDto[] = [];

  let currentHash: string | undefined;
  let currentFinalLine = 0;
  let currentOriginalLine = 0;

  let pendingName: string | undefined;
  let pendingEmail: string | undefined;
  let pendingTime: number | undefined;
  let pendingTz: number | undefined;

  for (const line of raw.split("\n")) {
    if (line.length === 0) {
      continue;
    }

    if (line.startsWith("\t")) {
      const content = line.slice(1);
      if (currentHash === undefined) {
        throw new GitParseError("Blame content line without a preceding header.");
      }
      const meta = metadata.get(currentHash);
      if (meta === undefined) {
        throw new GitParseError("Blame content line references unknown commit metadata.");
      }
      lines.push({
        finalLine: currentFinalLine,
        originalLine: currentOriginalLine,
        commit: currentHash,
        author: meta.author,
        timestamp: meta.timestamp,
        content,
        origin: ZERO_HASH_PATTERN.test(currentHash) ? "local" : "committed",
      });
      continue;
    }

    if (line.startsWith("author ")) {
      pendingName = line.slice("author ".length);
    } else if (line.startsWith("author-mail ")) {
      pendingEmail = stripAngleBrackets(line.slice("author-mail ".length));
    } else if (line.startsWith("author-time ")) {
      const seconds = Number(line.slice("author-time ".length));
      if (!Number.isInteger(seconds)) {
        throw new GitParseError("Blame author-time was not numeric.");
      }
      pendingTime = seconds;
    } else if (line.startsWith("author-tz ")) {
      pendingTz = parseUtcOffset(line.slice("author-tz ".length));
    } else if (isBlameHeaderLine(line)) {
      const header = parseBlameHeader(line);
      const isNewCommit = !metadata.has(header.hash);
      currentHash = header.hash;
      currentOriginalLine = header.originalLine;
      currentFinalLine = header.finalLine;
      if (isNewCommit) {
        // A header for a commit not seen before starts a fresh metadata
        // block: drop anything half-collected, so a truncated previous
        // block can never bleed its author into this commit.
        pendingName = undefined;
        pendingEmail = undefined;
        pendingTime = undefined;
        pendingTz = undefined;
      }
    }
    // Other metadata lines (`committer*`, `summary`, `previous`,
    // `filename`, `boundary`) carry nothing `BlameLineDto` needs and are
    // ignored without erroring — an unknown key must never fail a blame.

    if (
      currentHash !== undefined &&
      pendingName !== undefined &&
      pendingEmail !== undefined &&
      pendingTime !== undefined &&
      pendingTz !== undefined &&
      !metadata.has(currentHash)
    ) {
      metadata.set(currentHash, {
        author: { name: pendingName, email: pendingEmail },
        timestamp: { secondsSinceEpoch: pendingTime, utcOffsetMinutes: pendingTz },
      });
    }
  }

  return lines;
}

function stripAngleBrackets(raw: string): string {
  return raw.replace(/^<+/, "").replace(/>+$/, "");
}

/** A blame header is `<hash> <origLine> <finalLine> [<count>]`. Told apart
 * from a metadata line (`author ...`, `summary ...`) by requiring the first
 * token to be all hex and the next two to be integers — the same
 * discrimination `is_blame_header_line` makes in Rust. */
function isBlameHeaderLine(line: string): boolean {
  const parts = line.split(" ");
  if (parts.length < 3) {
    return false;
  }
  if (parts[0].length === 0 || !/^[0-9a-fA-F]+$/.test(parts[0])) {
    return false;
  }
  return /^\d+$/.test(parts[1]) && /^\d+$/.test(parts[2]);
}

function parseBlameHeader(line: string): {
  hash: string;
  originalLine: number;
  finalLine: number;
} {
  const parts = line.split(" ");
  const originalLine = Number(parts[1]);
  const finalLine = Number(parts[2]);
  if (!Number.isInteger(originalLine) || !Number.isInteger(finalLine)) {
    throw new GitParseError("Blame line numbers were not numeric.");
  }
  return { hash: parts[0], originalLine, finalLine };
}
