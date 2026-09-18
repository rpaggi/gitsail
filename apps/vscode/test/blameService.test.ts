import { describe, expect, it, vi } from "vitest";

import { BlameQueryCache, CommitSubjectCache, getBlame, resolveCommitSubjects } from "../src/blameService";
import { Envelope } from "../src/protocol";

function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

function ok<T>(data: T): Envelope<T> {
  return { status: "ok", schemaVersion: 1, requestId: "req-1", data };
}

describe("getBlame", () => {
  it("blames the working tree when no revision is given", async () => {
    const run = vi.fn(async () => ok({ file: "a.ts", revision: null, lines: [] }));
    await getBlame(stubClient(run), { repoRoot: "/repo", filePath: "a.ts" });
    expect(run).toHaveBeenCalledWith(["blame", "--repo", "/repo", "a.ts"], undefined);
  });

  it("includes --revision when given", async () => {
    const run = vi.fn(async () => ok({ file: "a.ts", revision: "HEAD", lines: [] }));
    await getBlame(stubClient(run), { repoRoot: "/repo", filePath: "a.ts", revision: "HEAD" });
    expect(run).toHaveBeenCalledWith(["blame", "--repo", "/repo", "a.ts", "--revision", "HEAD"], undefined);
  });
});

describe("BlameQueryCache (US-034 criterion 1 pattern, client-side)", () => {
  it("dedupes repeated queries for the same file/revision/contentVersion", async () => {
    const run = vi.fn(async () => ok({ file: "a.ts", revision: null, lines: [] }));
    const cache = new BlameQueryCache(stubClient(run));
    const query = { repoRoot: "/repo", filePath: "a.ts" };

    await cache.get(query, 1);
    await cache.get(query, 1);

    expect(run).toHaveBeenCalledTimes(1);
  });

  it("a changed content version re-queries instead of serving stale blame across an edit", async () => {
    const run = vi.fn(async () => ok({ file: "a.ts", revision: null, lines: [] }));
    const cache = new BlameQueryCache(stubClient(run));
    const query = { repoRoot: "/repo", filePath: "a.ts" };

    await cache.get(query, 1);
    await cache.get(query, 2);

    expect(run).toHaveBeenCalledTimes(2);
  });

  it("invalidateAll() forces a fresh query even for a previously cached key", async () => {
    const run = vi.fn(async () => ok({ file: "a.ts", revision: null, lines: [] }));
    const cache = new BlameQueryCache(stubClient(run));
    const query = { repoRoot: "/repo", filePath: "a.ts" };

    await cache.get(query, 1);
    cache.invalidateAll();
    await cache.get(query, 1);

    expect(run).toHaveBeenCalledTimes(2);
  });
});

describe("CommitSubjectCache / resolveCommitSubjects", () => {
  it("fetches each distinct hash only once", async () => {
    const run = vi.fn(async () => ok({ hash: "a".repeat(40), subject: "Fix the bug" }));
    const cache = new CommitSubjectCache(stubClient(run));

    await cache.get("/repo", "a".repeat(40));
    await cache.get("/repo", "a".repeat(40));

    expect(run).toHaveBeenCalledTimes(1);
  });

  it("resolveCommitSubjects is best-effort: one failing hash never blocks the others", async () => {
    const run = vi.fn(async (args: readonly string[]) => {
      const hash = args[args.length - 1];
      if (hash === "b".repeat(40)) {
        return {
          status: "error" as const,
          schemaVersion: 1,
          requestId: "req-1",
          error: { code: "repository_not_found", message: "no such commit" },
        };
      }
      return ok({ hash, subject: `subject for ${hash}` });
    });
    const cache = new CommitSubjectCache(stubClient(run));

    const subjects = await resolveCommitSubjects(cache, "/repo", ["a".repeat(40), "b".repeat(40)]);

    expect(subjects.get("a".repeat(40))).toBe(`subject for ${"a".repeat(40)}`);
    expect(subjects.has("b".repeat(40))).toBe(false);
  });
});
