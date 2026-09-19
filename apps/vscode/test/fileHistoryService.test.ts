// File history service, against real repositories.
//
// The three pre-ADR-025 tests here asserted the `gitsail log` argument list
// (bare query / revision+limit+cursor / `--no-follow`). Each is restated
// below as the behavior it stood for, so the same three properties are
// still covered: a bare query returns the file's history, a cursor resumes
// it, and `followRenames: false` stops at a rename boundary.
// `describeFileHistoryOutcome` is pure and unchanged.

import { describe, expect, it } from "vitest";

import { describeFileHistoryOutcome, getFileHistoryPage } from "../src/fileHistoryService";
import { GitClient } from "../src/git/gitClient";
import { TempRepo } from "./support/tempRepo";

const client = new GitClient();

describe("getFileHistoryPage (US-074 criterion 1: preserves file + revision context)", () => {
  it("returns a file's own history for a bare query", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("src/lib.rs", "one\n");
      const first = repo.commit("first");
      repo.write("unrelated.rs", "x\n");
      repo.commit("unrelated");

      const result = await getFileHistoryPage(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
      });

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      // Only the commit that touched this file, never the unrelated one.
      expect(result.value.items.map((c) => c.hash)).toEqual([first]);
    } finally {
      repo.dispose();
    }
  });

  it("honors the revision, limit and cursor so pagination stays continuous", async () => {
    const repo = TempRepo.create();
    try {
      const hashes: string[] = [];
      for (let i = 0; i < 4; i += 1) {
        repo.write("src/lib.rs", `rev ${i}\n`);
        hashes.push(repo.commit(`commit ${i}`));
      }
      const newestFirst = [...hashes].reverse();

      const first = await getFileHistoryPage(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        revision: "main",
        limit: 2,
      });
      expect(first.kind).toBe("ok");
      if (first.kind !== "ok") return;
      expect(first.value.items.map((c) => c.hash)).toEqual(newestFirst.slice(0, 2));

      const second = await getFileHistoryPage(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        revision: "main",
        limit: 2,
        cursor: first.value.nextCursor,
      });
      expect(second.kind).toBe("ok");
      if (second.kind !== "ok") return;
      expect(second.value.items.map((c) => c.hash)).toEqual(newestFirst.slice(2, 4));
    } finally {
      repo.dispose();
    }
  });

  it("stops at a rename boundary only when explicitly asked to", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("old.rs", "contents long enough for git to detect the rename\n");
      // A placeholder so `src/` exists for `git mv` to move into.
      repo.write("src/.keep", "");
      const first = repo.commit("first");
      repo.git(["mv", "old.rs", "src/lib.rs"]);
      const renamed = repo.commit("rename");

      const following = await getFileHistoryPage(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
      });
      expect(following.kind).toBe("ok");
      if (following.kind !== "ok") return;
      expect(following.value.items.map((c) => c.hash)).toEqual([renamed, first]);

      const stopped = await getFileHistoryPage(client, {
        repoRoot: repo.root,
        filePath: "src/lib.rs",
        followRenames: false,
      });
      expect(stopped.kind).toBe("ok");
      if (stopped.kind !== "ok") return;
      expect(stopped.value.items.map((c) => c.hash)).toEqual([renamed]);
    } finally {
      repo.dispose();
    }
  });
});

describe("describeFileHistoryOutcome (US-074 criterion 3: explicit empty state)", () => {
  it("reports an explicit empty state for a file with no history", () => {
    expect(describeFileHistoryOutcome({ items: [], hasMore: false })).toEqual({ kind: "empty" });
  });

  it("reports entries when the page has at least one commit", () => {
    const page = { items: [{ hash: "a".repeat(40) } as never], hasMore: false };
    expect(describeFileHistoryOutcome(page)).toEqual({ kind: "entries", page });
  });
});
