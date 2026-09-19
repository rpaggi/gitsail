// Line history service, against real repositories.
//
// The two pre-ADR-025 tests asserted the `gitsail line-history` argument
// list (implied HEAD / explicit `--revision`); both are restated below as
// the behavior they stood for. `describeLineHistoryBufferCaveat` is pure
// and unchanged.

import { describe, expect, it } from "vitest";

import { describeLineHistoryBufferCaveat, getLineHistory } from "../src/lineHistoryService";
import { GitClient } from "../src/git/gitClient";
import { TempRepo } from "./support/tempRepo";

const client = new GitClient();

describe("getLineHistory (US-075 criterion 1: uses the current selection and revision)", () => {
  it("traces the selected range from HEAD when no revision is given", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("src/lib.rs", "one\ntwo\nthree\n");
      const first = repo.commit("first");
      repo.write("src/lib.rs", "one\nTWO\nthree\n");
      const second = repo.commit("second");

      const result = await getLineHistory(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        range: { startLine: 2, endLine: 2 },
      });

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.range).toEqual({ start: 2, end: 2 });
      // The implied HEAD is echoed back as the commit it actually resolved
      // to, never as the literal string "HEAD".
      expect(result.value.revision).toBe(second);
      expect(result.value.entries.map((e) => e.commit.hash)).toEqual([second, first]);
    } finally {
      repo.dispose();
    }
  });

  it("traces from an explicitly given revision instead of HEAD", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("src/lib.rs", "one\ntwo\n");
      const first = repo.commit("first");
      repo.write("src/lib.rs", "one\nTWO\n");
      repo.commit("second");

      const result = await getLineHistory(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        range: { startLine: 2, endLine: 2 },
        revision: first,
      });

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.revision).toBe(first);
      // Tracing from `first` must not report the later commit.
      expect(result.value.entries.map((e) => e.commit.hash)).toEqual([first]);
    } finally {
      repo.dispose();
    }
  });

  it("returns an error result for an invalid range rather than querying something else", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("src/lib.rs", "one\n");
      repo.commit("first");

      const result = await getLineHistory(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        range: { startLine: 9, endLine: 2 },
      });

      expect(result.kind).toBe("error");
    } finally {
      repo.dispose();
    }
  });
});

describe("describeLineHistoryBufferCaveat (US-075 criterion 3)", () => {
  it("warns about disk-vs-buffer line drift only when the document is dirty", () => {
    expect(describeLineHistoryBufferCaveat(true)).toMatch(/unsaved/i);
    expect(describeLineHistoryBufferCaveat(true)).toMatch(/disk/i);
    expect(describeLineHistoryBufferCaveat(false)).toBeUndefined();
  });
});
