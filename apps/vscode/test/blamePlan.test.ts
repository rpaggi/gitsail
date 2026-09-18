import { describe, expect, it } from "vitest";

import { distinctCommitHashesForTarget, planBlameDecorations } from "../src/blamePlan";
import { DEFAULT_BLAME_DATE_STYLE, DEFAULT_BLAME_FORMAT, DEFAULT_BLAME_MODE } from "../src/blameFormat";
import { BlameLineDto } from "../src/dto";

const config = {
  enabled: true,
  format: DEFAULT_BLAME_FORMAT,
  mode: DEFAULT_BLAME_MODE,
  delayMs: 0,
  dateStyle: DEFAULT_BLAME_DATE_STYLE,
};

function line(finalLine: number, commit: string, origin: "committed" | "local" = "committed"): BlameLineDto {
  return {
    finalLine,
    originalLine: finalLine,
    commit,
    author: { name: "Ada", email: "ada@example.com" },
    timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    content: `line ${finalLine}`,
    origin,
  };
}

const hashA = "a".repeat(40);
const hashB = "b".repeat(40);

describe("distinctCommitHashesForTarget", () => {
  it("returns only the hashes of the lines within a single-line target", () => {
    const lines = [line(1, hashA), line(2, hashB), line(3, hashA)];
    expect(distinctCommitHashesForTarget(lines, { mode: "currentLine", line: 2 })).toEqual([hashB]);
  });

  it("de-duplicates across a multi-range target", () => {
    const lines = [line(1, hashA), line(2, hashA), line(3, hashB), line(4, hashB)];
    const hashes = distinctCommitHashesForTarget(lines, {
      mode: "allVisibleLines",
      ranges: [{ startLine: 1, endLine: 4 }],
    });
    expect(new Set(hashes)).toEqual(new Set([hashA, hashB]));
  });

  it("never asks for a subject for an uncommitted (local) line", () => {
    const lines = [line(1, "0".repeat(40), "local")];
    expect(distinctCommitHashesForTarget(lines, { mode: "currentLine", line: 1 })).toEqual([]);
  });
});

describe("planBlameDecorations (US-072 criterion 2: current-line vs. all-visible-lines)", () => {
  it("current-line mode decorates exactly one line, even when many are blamed", () => {
    const lines = [line(1, hashA), line(2, hashB), line(3, hashA)];
    const plan = planBlameDecorations(lines, config, { mode: "currentLine", line: 2 }, new Map(), false);
    expect(plan.map((p) => p.line)).toEqual([2]);
  });

  it("all-visible-lines mode decorates every line across all given ranges", () => {
    const lines = [line(1, hashA), line(2, hashA), line(5, hashB), line(6, hashB)];
    const plan = planBlameDecorations(
      lines,
      config,
      { mode: "allVisibleLines", ranges: [{ startLine: 1, endLine: 2 }, { startLine: 5, endLine: 6 }] },
      new Map(),
      false,
    );
    expect(plan.map((p) => p.line).sort()).toEqual([1, 2, 5, 6]);
  });

  it("resolves ${message} from the supplied per-hash subject map, never from the blamed source line", () => {
    const lines = [line(1, hashA)];
    const plan = planBlameDecorations(
      lines,
      config,
      { mode: "currentLine", line: 1 },
      new Map([[hashA, "Add the parser"]]),
      false,
    );
    expect(plan[0].text.contentText).toContain("Add the parser");
    expect(plan[0].text.contentText).not.toContain("line 1");
  });

  it("a line outside the target is never included, so an edit invalidating the plan cannot leave a stray decoration", () => {
    const lines = [line(1, hashA), line(2, hashA)];
    const plan = planBlameDecorations(lines, config, { mode: "currentLine", line: 1 }, new Map(), false);
    expect(plan).toHaveLength(1);
    expect(plan[0].line).toBe(1);
  });
});
