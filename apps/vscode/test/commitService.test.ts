import { describe, expect, it, vi } from "vitest";

import { describeCommitDiffBase, getCommit, getCommitDiff, getFileContentAtRevision } from "../src/commitService";
import { CommitDiffDto } from "../src/dto";
import { Envelope } from "../src/protocol";

function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

function ok<T>(data: T): Envelope<T> {
  return { status: "ok", schemaVersion: 1, requestId: "req-1", data };
}

describe("getCommit", () => {
  it("calls `commit --repo <root> <revision>`", async () => {
    const run = vi.fn(async () => ok({ hash: "a".repeat(40) }));
    await getCommit(stubClient(run), "/repo", "HEAD~2");
    expect(run).toHaveBeenCalledWith(["commit", "--repo", "/repo", "HEAD~2"], undefined);
  });
});

describe("getCommitDiff", () => {
  it("calls `commit-diff --repo <root> <revision>`", async () => {
    const run = vi.fn(async () => ok({ target: "a".repeat(40), base: null, diff: { files: [] } }));
    await getCommitDiff(stubClient(run), "/repo", "abc123");
    expect(run).toHaveBeenCalledWith(["commit-diff", "--repo", "/repo", "abc123"], undefined);
  });
});

describe("getFileContentAtRevision", () => {
  it("calls `show-file --repo <root> <path> --revision <revision>`", async () => {
    const run = vi.fn(async () => ok({ kind: "text", path: "a.txt", revision: "abc", content: "hi" }));
    await getFileContentAtRevision(stubClient(run), "/repo", "src/a.txt", "abc123");
    expect(run).toHaveBeenCalledWith(
      ["show-file", "--repo", "/repo", "src/a.txt", "--revision", "abc123"],
      undefined,
    );
  });
});

describe("describeCommitDiffBase (US-076 criterion 1)", () => {
  it("names the empty tree for a root commit", () => {
    const commitDiff: CommitDiffDto = { target: "a".repeat(40), base: null, diff: { files: [] } };
    expect(describeCommitDiffBase(commitDiff)).toMatch(/root commit/i);
    expect(describeCommitDiffBase(commitDiff)).toMatch(/empty tree/i);
  });

  it("names the exact first-parent hash for a non-root commit, never a vaguer description", () => {
    const base = "b".repeat(40);
    const commitDiff: CommitDiffDto = { target: "a".repeat(40), base, diff: { files: [] } };
    expect(describeCommitDiffBase(commitDiff)).toContain(base);
    expect(describeCommitDiffBase(commitDiff)).toMatch(/first parent/i);
  });
});
