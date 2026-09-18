// Pure diff-presentation logic (US-057): both the unified and
// side-by-side views are derived, here, from the exact same `DiffHunkDto`
// the Core returned — neither view recomputes or reinterprets a hunk,
// they only lay the same lines out differently. Kept as plain functions
// (no Vue) so they can be unit-tested directly, following this project's
// established `commitGraphLayout.ts` convention of extracting presentation
// logic out of `.vue` files.

import type { DiffHunkDto, DiffLineOriginDto } from "../services/dto";

export interface UnifiedLine {
  origin: DiffLineOriginDto;
  content: string;
  oldLineNumber: number | null;
  newLineNumber: number | null;
}

/** Flattens one hunk into its unified-diff line list, with old/new line
 * numbers advancing exactly as Git's own unified format defines: context
 * advances both counters, a deletion only the old one, an addition only
 * the new one (US-057 criterion 2: "linhas e hunks mapeiam corretamente
 * adições/remoções"). */
export function unifiedLines(hunk: DiffHunkDto): UnifiedLine[] {
  let oldLine = hunk.oldStart;
  let newLine = hunk.newStart;
  const result: UnifiedLine[] = [];
  for (const line of hunk.lines) {
    if (line.origin === "context") {
      result.push({ origin: line.origin, content: line.content, oldLineNumber: oldLine, newLineNumber: newLine });
      oldLine += 1;
      newLine += 1;
    } else if (line.origin === "deletion") {
      result.push({ origin: line.origin, content: line.content, oldLineNumber: oldLine, newLineNumber: null });
      oldLine += 1;
    } else {
      result.push({ origin: line.origin, content: line.content, oldLineNumber: null, newLineNumber: newLine });
      newLine += 1;
    }
  }
  return result;
}

export interface SideBySideCell {
  lineNumber: number;
  content: string;
  changed: boolean;
}

export interface SideBySideRow {
  left: SideBySideCell | null;
  right: SideBySideCell | null;
}

/**
 * Pairs a hunk's lines into side-by-side rows (US-057 criterion 1: the
 * two modes must show the same content, just laid out differently). A
 * context line occupies one row on both sides unchanged; a run of
 * deletions immediately followed by a run of additions (the shape every
 * unified diff hunk actually has, by construction) is zipped
 * position-by-position into rows, left/right padded with `null` when the
 * two runs have different lengths — never fabricating a paired line that
 * was not in the original hunk.
 */
export function sideBySideRows(hunk: DiffHunkDto): SideBySideRow[] {
  const rows: SideBySideRow[] = [];
  let oldLine = hunk.oldStart;
  let newLine = hunk.newStart;
  let i = 0;
  const lines = hunk.lines;

  while (i < lines.length) {
    const line = lines[i];
    if (line.origin === "context") {
      rows.push({
        left: { lineNumber: oldLine, content: line.content, changed: false },
        right: { lineNumber: newLine, content: line.content, changed: false },
      });
      oldLine += 1;
      newLine += 1;
      i += 1;
      continue;
    }

    const deletions: string[] = [];
    while (i < lines.length && lines[i].origin === "deletion") {
      deletions.push(lines[i].content);
      i += 1;
    }
    const additions: string[] = [];
    while (i < lines.length && lines[i].origin === "addition") {
      additions.push(lines[i].content);
      i += 1;
    }

    const pairCount = Math.max(deletions.length, additions.length);
    for (let j = 0; j < pairCount; j += 1) {
      const deletion = deletions[j];
      const addition = additions[j];
      rows.push({
        left: deletion !== undefined ? { lineNumber: oldLine, content: deletion, changed: true } : null,
        right: addition !== undefined ? { lineNumber: newLine, content: addition, changed: true } : null,
      });
      if (deletion !== undefined) oldLine += 1;
      if (addition !== undefined) newLine += 1;
    }
  }

  return rows;
}
