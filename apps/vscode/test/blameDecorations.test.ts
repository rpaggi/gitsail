import { describe, expect, it } from "vitest";

import {
  BLAME_ANNOTATION_COLOR,
  BLAME_ANNOTATION_MARGIN,
  buildBlameDecorationLayout,
  normalizeBlameAnchorColumn,
} from "../src/blameDecorations";

describe("blame decoration layout (T-268)", () => {
  it("anchors the range at the end of the line, not at column 0", () => {
    const layout = buildBlameDecorationLayout(
      [{ line: 3, contentText: "Ada, 2026-09-19 • abc123 • fix" }],
      () => 12,
    );

    expect(layout).toHaveLength(1);
    expect(layout[0]?.zeroBasedLine).toBe(2);
    expect(layout[0]?.startChar).toBe(12);
    expect(layout[0]?.endChar).toBe(12);
  });

  it("anchors an empty line at column 0 without extra handling", () => {
    const layout = buildBlameDecorationLayout([{ line: 1, contentText: "Ada • empty" }], () => 0);

    expect(layout[0]?.zeroBasedLine).toBe(0);
    expect(layout[0]?.startChar).toBe(0);
    expect(layout[0]?.endChar).toBe(0);
  });

  it("does not prefix the annotation with padding spaces (margin owns the spacing)", () => {
    const layout = buildBlameDecorationLayout([{ line: 2, contentText: "Ada • note" }], () => 5);

    expect(layout[0]?.contentText).toBe("Ada • note");
  });

  it("maps each entry to its own line length", () => {
    const lengths = new Map([
      [0, 4],
      [4, 0],
      [9, 30],
    ]);
    const layout = buildBlameDecorationLayout(
      [
        { line: 1, contentText: "a" },
        { line: 5, contentText: "b" },
        { line: 10, contentText: "c" },
      ],
      (zeroBasedLine) => lengths.get(zeroBasedLine) ?? 0,
    );

    expect(layout.map((entry) => [entry.zeroBasedLine, entry.startChar, entry.endChar])).toEqual([
      [0, 4, 4],
      [4, 0, 0],
      [9, 30, 30],
    ]);
  });

  it("keeps a single gap between code and annotation, owned by the margin constant", () => {
    expect(BLAME_ANNOTATION_MARGIN).toBe("0 0 0 1.5em");
  });

  it("renders the annotation in neutral gray at 50% opacity", () => {
    expect(BLAME_ANNOTATION_COLOR).toBe("rgba(128, 128, 128, 0.5)");
  });

  it("normalizes unsafe line lengths to a safe column", () => {
    expect(normalizeBlameAnchorColumn(-3)).toBe(0);
    expect(normalizeBlameAnchorColumn(Number.NaN)).toBe(0);
    expect(normalizeBlameAnchorColumn(Number.POSITIVE_INFINITY)).toBe(0);
    expect(normalizeBlameAnchorColumn(7.9)).toBe(7);
  });
});
