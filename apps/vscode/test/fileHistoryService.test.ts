import { describe, expect, it, vi } from "vitest";

import { describeFileHistoryOutcome, getFileHistoryPage } from "../src/fileHistoryService";
import { Envelope } from "../src/protocol";

function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

function ok<T>(data: T): Envelope<T> {
  return { status: "ok", schemaVersion: 1, requestId: "req-1", data };
}

describe("getFileHistoryPage (US-074 criterion 1: preserves file + revision context)", () => {
  it("builds a bare `log --path` query with no revision/cursor", async () => {
    const run = vi.fn(async () => ok({ items: [], hasMore: false }));
    await getFileHistoryPage(stubClient(run), { repoRoot: "/repo", filePath: "src/lib.rs" });
    expect(run).toHaveBeenCalledWith(["log", "--repo", "/repo", "--path", "src/lib.rs"], undefined);
  });

  it("includes the revision, limit, and cursor when given (pagination continuity)", async () => {
    const run = vi.fn(async () => ok({ items: [], hasMore: false }));
    await getFileHistoryPage(stubClient(run), {
      repoRoot: "/repo",
      filePath: "src/lib.rs",
      revision: "main",
      limit: 20,
      cursor: "20",
    });
    expect(run).toHaveBeenCalledWith(
      ["log", "--repo", "/repo", "main", "--path", "src/lib.rs", "--limit", "20", "--cursor", "20"],
      undefined,
    );
  });

  it("passes --no-follow only when explicitly asked to stop at rename boundaries", async () => {
    const run = vi.fn(async () => ok({ items: [], hasMore: false }));
    await getFileHistoryPage(stubClient(run), {
      repoRoot: "/repo",
      filePath: "src/lib.rs",
      followRenames: false,
    });
    expect(run).toHaveBeenCalledWith(
      ["log", "--repo", "/repo", "--path", "src/lib.rs", "--no-follow"],
      undefined,
    );
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
