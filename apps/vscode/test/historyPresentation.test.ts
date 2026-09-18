import { describe, expect, it } from "vitest";

import {
  LOAD_MORE_ITEM_ID,
  NO_HISTORY_ITEM_ID,
  buildFileHistoryQuickPickItems,
  buildLineHistoryQuickPickItems,
} from "../src/historyPresentation";
import { CommitDto, LineHistoryDto } from "../src/dto";
import { describeFileHistoryOutcome } from "../src/fileHistoryService";

function commit(overrides: Partial<CommitDto> = {}): CommitDto {
  return {
    hash: "a".repeat(40),
    shortHash: "aaaaaaaa",
    parents: [],
    author: { name: "Ada Lovelace", email: "ada@example.com" },
    committer: { name: "Ada Lovelace", email: "ada@example.com" },
    authorDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    subject: "Fix the bug",
    body: "",
    decorations: [],
    isMerge: false,
    isRoot: false,
    ...overrides,
  };
}

describe("buildFileHistoryQuickPickItems (US-074 criteria 1 and 3)", () => {
  it("shows an explicit 'no history' row for an empty page, never a bare empty list", () => {
    const items = buildFileHistoryQuickPickItems(describeFileHistoryOutcome({ items: [], hasMore: false }));
    expect(items).toHaveLength(1);
    expect(items[0].id).toBe(NO_HISTORY_ITEM_ID);
  });

  it("renders one row per commit, with hash/subject/author/date", () => {
    const outcome = describeFileHistoryOutcome({ items: [commit()], hasMore: false });
    const items = buildFileHistoryQuickPickItems(outcome);
    expect(items).toHaveLength(1);
    expect(items[0].id).toBe("a".repeat(40));
    expect(items[0].label).toContain("aaaaaaaa");
    expect(items[0].label).toContain("Fix the bug");
    expect(items[0].description).toBe("Ada Lovelace");
  });

  it("appends a 'load more' row carrying the pagination cursor when more pages exist", () => {
    const outcome = describeFileHistoryOutcome({ items: [commit()], hasMore: true, nextCursor: "50" });
    const items = buildFileHistoryQuickPickItems(outcome);
    expect(items).toHaveLength(2);
    expect(items[1].id).toBe(LOAD_MORE_ITEM_ID);
    expect(items[1].description).toBe("50");
  });

  it("only truncates the subject's first line, keeping the row single-line", () => {
    const outcome = describeFileHistoryOutcome({
      items: [commit({ subject: "Fix the bug\n\nLonger explanation here" })],
      hasMore: false,
    });
    expect(buildFileHistoryQuickPickItems(outcome)[0].label).not.toContain("Longer explanation");
  });
});

describe("buildLineHistoryQuickPickItems (US-075 criterion 2)", () => {
  it("shows an explicit 'no history' row for an empty range history", () => {
    const history: LineHistoryDto = {
      file: "src/lib.rs",
      revision: "a".repeat(40),
      range: { start: 10, end: 20 },
      entries: [],
    };
    const items = buildLineHistoryQuickPickItems(history);
    expect(items).toHaveLength(1);
    expect(items[0].id).toBe(NO_HISTORY_ITEM_ID);
    expect(items[0].detail).toContain("10-20");
  });

  it("renders one row per entry, mentioning how many lines that commit touched in range", () => {
    const history: LineHistoryDto = {
      file: "src/lib.rs",
      revision: "a".repeat(40),
      range: { start: 10, end: 20 },
      entries: [
        {
          commit: commit(),
          hunks: [
            {
              oldStart: 10,
              oldLines: 1,
              newStart: 10,
              newLines: 2,
              lines: [
                { origin: "addition", content: "x", hasTrailingNewline: true },
                { origin: "addition", content: "y", hasTrailingNewline: true },
              ],
            },
          ],
        },
      ],
    };
    const items = buildLineHistoryQuickPickItems(history);
    expect(items).toHaveLength(1);
    expect(items[0].id).toBe("a".repeat(40));
    expect(items[0].detail).toContain("2 line(s) touched in this range");
  });
});
