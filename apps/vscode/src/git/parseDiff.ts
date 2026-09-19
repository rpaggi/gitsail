// Unified-patch parsing into `DiffDto`. Pure.
//
// Mirrors `crates/gitsail-git/src/provider.rs`'s
// `parse_diff`/`parse_diff_block`/`parse_diff_hunk`, including its
// documented limitations (the ` and ` split for binary paths, the last
// ` b/` split for the `diff --git` header) — matching the Core's exact
// behavior, imperfections included, is what keeps the two from rendering a
// different diff for the same commit.

import { GitParseError } from "./errors";
import {
  ChangeTypeDto,
  DiffDto,
  DiffHunkDto,
  DiffLineDto,
  DiffLineOriginDto,
  FileDiffDto,
} from "../dto";

/** Per-file cap on parsed hunk content, independent of the process-wide
 * stream cap in `process.ts`: one huge file should not be materialized into
 * the extension host's memory just because the overall diff fit under the
 * stream cap. Hunks past the cap are withheld and `truncated` is set,
 * rather than presenting partial hunk data as complete. Same value as
 * `gitsail-git`'s `MAX_FILE_DIFF_BYTES`. */
const MAX_FILE_DIFF_BYTES = 512 * 1024;

/** Splits on `\n` without stripping a preceding `\r`, unlike a naive
 * `split(/\r?\n/)`: a CRLF file's content lines must keep their `\r` so it
 * round-trips through `DiffLineDto.content`. A single trailing empty
 * element from a final `\n` is dropped. */
export function rawLines(raw: string): string[] {
  const lines = raw.split("\n");
  if (lines.length > 0 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}

export function parseDiff(raw: string): DiffDto {
  return { files: splitDiffBlocks(raw).map(parseDiffBlock) };
}

/** Splits a unified patch into per-file blocks, each starting at its
 * `diff --git a/... b/...` header. Anything before the first header is
 * discarded rather than misread as content belonging to no file. */
function splitDiffBlocks(raw: string): string[][] {
  const blocks: string[][] = [];
  for (const line of rawLines(raw)) {
    if (line.startsWith("diff --git ")) {
      blocks.push([line]);
    } else if (blocks.length > 0) {
      blocks[blocks.length - 1].push(line);
    }
  }
  return blocks;
}

function parseDiffBlock(lines: string[]): FileDiffDto {
  const header = lines[0];
  if (header === undefined) {
    throw new GitParseError("Empty diff block.");
  }

  let previousPath: string | undefined;
  let isCopy = false;
  let isNewFile = false;
  let isDeletedFile = false;
  let isBinary = false;
  // `null` means the marker line was seen and pointed at `/dev/null`;
  // `undefined` means it was never seen at all (a pure binary or pure
  // mode-change diff). The distinction decides added-vs-deleted below.
  let markerOldPath: string | null | undefined;
  let markerNewPath: string | null | undefined;
  let binaryOldPath: string | undefined;
  let binaryNewPath: string | undefined;
  const hunks: DiffHunkDto[] = [];
  let hunkBytesTotal = 0;
  let truncated = false;

  let i = 1;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("rename from ")) {
      previousPath = line.slice("rename from ".length);
      i += 1;
    } else if (line.startsWith("rename to ")) {
      i += 1;
    } else if (line.startsWith("copy from ")) {
      previousPath = line.slice("copy from ".length);
      isCopy = true;
      i += 1;
    } else if (line.startsWith("copy to ")) {
      i += 1;
    } else if (line.startsWith("new file mode")) {
      isNewFile = true;
      i += 1;
    } else if (line.startsWith("deleted file mode")) {
      isDeletedFile = true;
      i += 1;
    } else if (line.startsWith("Binary files ") && line.endsWith(" differ")) {
      isBinary = true;
      const inner = line.slice("Binary files ".length, line.length - " differ".length);
      // Best-effort split on the first " and "; a path literally
      // containing that substring is a known, documented limitation
      // inherited from the Rust adapter.
      const sep = inner.indexOf(" and ");
      if (sep !== -1) {
        binaryOldPath = stripDiffPathPrefix(inner.slice(0, sep), "a/") ?? undefined;
        binaryNewPath = stripDiffPathPrefix(inner.slice(sep + " and ".length), "b/") ?? undefined;
      }
      i += 1;
    } else if (line.startsWith("--- ")) {
      markerOldPath = stripDiffPathPrefix(line.slice(4), "a/");
      i += 1;
    } else if (line.startsWith("+++ ")) {
      markerNewPath = stripDiffPathPrefix(line.slice(4), "b/");
      i += 1;
    } else if (line.startsWith("@@ ")) {
      const { hunk, consumed } = parseDiffHunk(lines, i);
      if (!truncated) {
        const hunkBytes = hunk.lines.reduce((sum, l) => sum + l.content.length + 1, 0);
        if (hunkBytesTotal + hunkBytes > MAX_FILE_DIFF_BYTES) {
          truncated = true;
        } else {
          hunkBytesTotal += hunkBytes;
          hunks.push(hunk);
        }
      }
      i += consumed;
    } else {
      i += 1;
    }
  }

  const [headerOld, headerNew] = parseDiffGitHeader(header);
  const oldPath = markerOldPath ?? binaryOldPath ?? headerOld;
  const newPath = markerNewPath ?? binaryNewPath ?? headerNew;
  const oldIsDevNull = markerOldPath === null;
  const newIsDevNull = markerNewPath === null;

  let changeType: ChangeTypeDto;
  let path: string;
  let resolvedPreviousPath: string | null = null;
  if (previousPath !== undefined) {
    changeType = isCopy ? "copied" : "renamed";
    path = newPath;
    resolvedPreviousPath = previousPath;
  } else if (isNewFile || oldIsDevNull) {
    changeType = "added";
    path = newPath;
  } else if (isDeletedFile || newIsDevNull) {
    changeType = "deleted";
    path = oldPath;
  } else {
    changeType = "modified";
    path = newPath;
  }

  return { path, previousPath: resolvedPreviousPath, changeType, isBinary, truncated, hunks };
}

/** Strips `a/`/`b/` from a `---`/`+++` marker path. Git appends a trailing
 * tab to disambiguate a filename containing whitespace from the omitted
 * legacy timestamp — stripped here so it never leaks into a parsed path.
 * Returns `null` for `/dev/null`. */
function stripDiffPathPrefix(raw: string, prefix: string): string | null {
  const withoutTab = raw.endsWith("\t") ? raw.slice(0, -1) : raw;
  if (withoutTab === "/dev/null") {
    return null;
  }
  return withoutTab.startsWith(prefix) ? withoutTab.slice(prefix.length) : withoutTab;
}

/** Fallback path extraction from `diff --git a/<old> b/<new>`, used only
 * when neither markers nor a `Binary files` line supplied paths (e.g. a
 * pure mode change). Splits on the last `" b/"`; a path containing that
 * exact substring is a known, documented limitation. */
function parseDiffGitHeader(line: string): [string, string] {
  if (!line.startsWith("diff --git ")) {
    throw new GitParseError("Diff block missing its 'diff --git' header.");
  }
  const rest = line.slice("diff --git ".length);
  if (!rest.startsWith("a/")) {
    throw new GitParseError("diff --git header missing its 'a/' prefix.");
  }
  const body = rest.slice(2);
  const sep = body.lastIndexOf(" b/");
  if (sep === -1) {
    throw new GitParseError("diff --git header missing its ' b/' separator.");
  }
  return [body.slice(0, sep), body.slice(sep + " b/".length)];
}

export function parseDiffHunk(
  lines: readonly string[],
  start: number,
): { hunk: DiffHunkDto; consumed: number } {
  const { oldStart, oldLines, newStart, newLines } = parseDiffHunkHeader(lines[start]);

  const contentLines: DiffLineDto[] = [];
  let i = start + 1;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("@@ ") || line.startsWith("diff --git ")) {
      break;
    }
    let origin: DiffLineOriginDto;
    switch (line[0]) {
      case " ":
        origin = "context";
        break;
      case "+":
        origin = "addition";
        break;
      case "-":
        origin = "deletion";
        break;
      default:
        // "\ No newline at end of file" refers to the content line
        // immediately preceding it; any other stray marker is skipped
        // without ending the hunk.
        if (line === "\\ No newline at end of file" && contentLines.length > 0) {
          contentLines[contentLines.length - 1].hasTrailingNewline = false;
        }
        i += 1;
        continue;
    }
    contentLines.push({ origin, content: line.slice(1), hasTrailingNewline: true });
    i += 1;
  }

  return {
    hunk: { oldStart, oldLines, newStart, newLines, lines: contentLines },
    consumed: i - start,
  };
}

/** Parses `@@ -old_start[,old_lines] +new_start[,new_lines] @@[ context]`.
 * A missing `,count` means a single-line range. */
function parseDiffHunkHeader(line: string): {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
} {
  if (!line.startsWith("@@ ")) {
    throw new GitParseError("Malformed diff hunk header.");
  }
  const rest = line.slice(3);
  const end = rest.indexOf(" @@");
  if (end === -1) {
    throw new GitParseError("Malformed diff hunk header: missing closing '@@'.");
  }
  const parts = rest.slice(0, end).split(" ");
  if (parts.length < 2) {
    throw new GitParseError("Diff hunk header missing a range.");
  }
  const [oldStart, oldLines] = parseDiffHunkRange(parts[0], "-");
  const [newStart, newLines] = parseDiffHunkRange(parts[1], "+");
  return { oldStart, oldLines, newStart, newLines };
}

function parseDiffHunkRange(range: string, sigil: string): [number, number] {
  if (!range.startsWith(sigil)) {
    throw new GitParseError("Diff hunk range missing its sigil.");
  }
  const body = range.slice(1);
  const comma = body.indexOf(",");
  const startText = comma === -1 ? body : body.slice(0, comma);
  if (startText.length === 0 || !/^\d+$/.test(startText)) {
    throw new GitParseError("Diff hunk range start was not numeric.");
  }
  if (comma === -1) {
    return [Number(startText), 1];
  }
  const countText = body.slice(comma + 1);
  if (!/^\d+$/.test(countText)) {
    throw new GitParseError("Diff hunk range count was not numeric.");
  }
  return [Number(startText), Number(countText)];
}
