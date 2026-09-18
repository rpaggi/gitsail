import { describe, expect, it } from "vitest";

import { sideBySideRows, unifiedLines } from "./diffPresentation";
import type { DiffHunkDto } from "../services/dto";

function multiHunkFixture(): DiffHunkDto {
  return {
    oldStart: 10,
    oldLines: 5,
    newStart: 10,
    newLines: 6,
    lines: [
      { origin: "context", content: "unchanged before", hasTrailingNewline: true },
      { origin: "deletion", content: "old line one", hasTrailingNewline: true },
      { origin: "deletion", content: "old line two", hasTrailingNewline: true },
      { origin: "addition", content: "new line one", hasTrailingNewline: true },
      { origin: "addition", content: "new line two", hasTrailingNewline: true },
      { origin: "addition", content: "new line three", hasTrailingNewline: true },
      { origin: "context", content: "unchanged after", hasTrailingNewline: true },
    ],
  };
}

function oldSideFromUnified(hunk: DiffHunkDto): string[] {
  return unifiedLines(hunk)
    .filter((l) => l.origin !== "addition")
    .map((l) => l.content);
}

function newSideFromUnified(hunk: DiffHunkDto): string[] {
  return unifiedLines(hunk)
    .filter((l) => l.origin !== "deletion")
    .map((l) => l.content);
}

function oldSideFromSideBySide(hunk: DiffHunkDto): string[] {
  return sideBySideRows(hunk)
    .map((r) => r.left)
    .filter((cell): cell is NonNullable<typeof cell> => cell !== null)
    .map((cell) => cell.content);
}

function newSideFromSideBySide(hunk: DiffHunkDto): string[] {
  return sideBySideRows(hunk)
    .map((r) => r.right)
    .filter((cell): cell is NonNullable<typeof cell> => cell !== null)
    .map((cell) => cell.content);
}

describe("unifiedLines", () => {
  it("advances old/new line numbers per Git's own unified-diff convention", () => {
    const lines = unifiedLines(multiHunkFixture());

    expect(lines[0]).toMatchObject({ origin: "context", oldLineNumber: 10, newLineNumber: 10 });
    expect(lines[1]).toMatchObject({ origin: "deletion", oldLineNumber: 11, newLineNumber: null });
    expect(lines[2]).toMatchObject({ origin: "deletion", oldLineNumber: 12, newLineNumber: null });
    expect(lines[3]).toMatchObject({ origin: "addition", oldLineNumber: null, newLineNumber: 11 });
    expect(lines[6]).toMatchObject({ origin: "context", oldLineNumber: 13, newLineNumber: 14 });
  });
});

describe("sideBySideRows", () => {
  it("pairs a run of deletions with a run of additions position-by-position", () => {
    const rows = sideBySideRows(multiHunkFixture());

    // context, then 3 paired rows (2 deletions vs 3 additions -> 3 rows,
    // the shorter side null-padded), then context.
    expect(rows).toHaveLength(5);
    expect(rows[0].left?.content).toBe("unchanged before");
    expect(rows[0].right?.content).toBe("unchanged before");
    expect(rows[1]).toEqual({
      left: { lineNumber: 11, content: "old line one", changed: true },
      right: { lineNumber: 11, content: "new line one", changed: true },
    });
    expect(rows[2]).toEqual({
      left: { lineNumber: 12, content: "old line two", changed: true },
      right: { lineNumber: 12, content: "new line two", changed: true },
    });
    expect(rows[3]).toEqual({
      left: null,
      right: { lineNumber: 13, content: "new line three", changed: true },
    });
    expect(rows[4].left?.content).toBe("unchanged after");
  });

  it("never fabricates a paired line beyond what the hunk actually contains", () => {
    const rows = sideBySideRows(multiHunkFixture());
    const totalCells = rows.reduce((sum, r) => sum + (r.left ? 1 : 0) + (r.right ? 1 : 0), 0);
    // 2 context lines shown on both sides (4 cells) + 2 deletions + 3
    // additions (5 cells) = 9 non-null cells total.
    expect(totalCells).toBe(9);
  });
});

describe("unified vs side-by-side equivalence (DoD fixture)", () => {
  it("both modes reconstruct the identical old-side and new-side content", () => {
    const hunk = multiHunkFixture();

    expect(oldSideFromSideBySide(hunk)).toEqual(oldSideFromUnified(hunk));
    expect(newSideFromSideBySide(hunk)).toEqual(newSideFromUnified(hunk));
  });

  it("holds across multiple hunks in the same file diff", () => {
    const secondHunk: DiffHunkDto = {
      oldStart: 50,
      oldLines: 1,
      newStart: 51,
      newLines: 1,
      lines: [
        { origin: "deletion", content: "removed entirely", hasTrailingNewline: true },
        { origin: "addition", content: "replacement", hasTrailingNewline: false },
      ],
    };

    for (const hunk of [multiHunkFixture(), secondHunk]) {
      expect(oldSideFromSideBySide(hunk)).toEqual(oldSideFromUnified(hunk));
      expect(newSideFromSideBySide(hunk)).toEqual(newSideFromUnified(hunk));
    }
  });
});
