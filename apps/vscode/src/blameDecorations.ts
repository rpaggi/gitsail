// Pure layout for inline blame decorations (T-268, EPIC-15).
//
// `extension.ts` renders each blame annotation as a VS Code `after`
// content-text decoration. An `after` decoration renders immediately after
// the end of its range, so the range must end at the end of the line —
// anchoring it at column 0 renders the annotation *before* the first code
// character and shoves the whole line right, which is the bug T-268 fixes.
//
// This module is deliberately `vscode`-free: it computes the zero-based
// line plus the anchor column from a caller-supplied line length, so unit
// tests can cover the anchoring rule without a VS Code host. Only
// `extension.ts` turns a `BlameDecorationLayout` into a real
// `vscode.Range`/`DecorationOptions`.

export interface BlameDecorationLayoutInput {
  /** 1-based line number, matching `BlameLineDto.finalLine` and
   * `BlameDecorationRenderEntry.line`. */
  line: number;
  contentText: string;
}

/** Horizontal gap between the last code character and the blame
 * annotation. Single-sourced here so the decoration type
 * (`createBlameDecorationType`) and every per-line call
 * (`buildBlameDecorationOptions`) stay identical — they must, because a
 * per-decoration `renderOptions.after` replaces the type's `after`
 * wholesale instead of merging into it. */
export const BLAME_ANNOTATION_MARGIN = "0 0 0 1.5em";

/** Annotation text color: neutral mid-gray at 50% opacity. A `ThemeColor`
 * cannot carry opacity, and every fixed theme token renders solid — so the
 * softness is done with alpha instead. Because half of the final pixel
 * comes from the editor background itself, the result stays neutral
 * relative to the background in any theme (dark, light, or custom),
 * instead of fighting it. */
export const BLAME_ANNOTATION_COLOR = "rgba(128, 128, 128, 0.5)";

export interface BlameDecorationLayout {
  /** 0-based line number for `vscode.Range`. */
  zeroBasedLine: number;
  /** Anchor column, always the full line length (0 for an empty line). */
  startChar: number;
  endChar: number;
  /** Annotation text verbatim — no leading padding. Horizontal spacing
   * comes from `BLAME_ANNOTATION_MARGIN` alone (applied both on the
   * decoration type and on every per-line call, since the latter replaces
   * the former), so prefixing spaces here would double the gap once the
   * anchor is correct. */
  contentText: string;
}

/** Normalizes a raw line length into a safe anchor column: negative,
// non-finite, or fractional lengths collapse to a non-negative integer. */
export function normalizeBlameAnchorColumn(lineLength: number): number {
  if (!Number.isFinite(lineLength)) {
    return 0;
  }
  return Math.max(0, Math.floor(lineLength));
}

export function buildBlameDecorationLayout(
  entries: readonly BlameDecorationLayoutInput[],
  getLineLength: (zeroBasedLine: number) => number,
): BlameDecorationLayout[] {
  return entries.map((entry) => {
    const zeroBasedLine = Math.max(0, entry.line - 1);
    const startChar = normalizeBlameAnchorColumn(getLineLength(zeroBasedLine));
    return {
      zeroBasedLine,
      startChar,
      endChar: startChar,
      contentText: entry.contentText,
    };
  });
}
