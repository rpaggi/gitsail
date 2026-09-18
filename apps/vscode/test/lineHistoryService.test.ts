import { describe, expect, it, vi } from "vitest";

import { describeLineHistoryBufferCaveat, getLineHistory } from "../src/lineHistoryService";
import { Envelope } from "../src/protocol";

function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

function ok<T>(data: T): Envelope<T> {
  return { status: "ok", schemaVersion: 1, requestId: "req-1", data };
}

describe("getLineHistory (US-075 criterion 1: uses the current selection and revision)", () => {
  it("builds `line-history <file> --range START-END` with no --revision when HEAD is implied", async () => {
    const run = vi.fn(async () => ok({ file: "src/lib.rs", revision: "a".repeat(40), range: { start: 10, end: 20 }, entries: [] }));
    await getLineHistory(stubClient(run), {
      repoRoot: "/repo",
      filePath: "src/lib.rs",
      range: { startLine: 10, endLine: 20 },
    });
    expect(run).toHaveBeenCalledWith(
      ["line-history", "--repo", "/repo", "src/lib.rs", "--range", "10-20"],
      undefined,
    );
  });

  it("includes an explicit --revision when one is given (e.g. the buffer's checked-out HEAD)", async () => {
    const run = vi.fn(async () => ok({ file: "src/lib.rs", revision: "main", range: { start: 1, end: 1 }, entries: [] }));
    await getLineHistory(stubClient(run), {
      repoRoot: "/repo",
      filePath: "src/lib.rs",
      range: { startLine: 1, endLine: 1 },
      revision: "main",
    });
    expect(run).toHaveBeenCalledWith(
      ["line-history", "--repo", "/repo", "src/lib.rs", "--range", "1-1", "--revision", "main"],
      undefined,
    );
  });
});

describe("describeLineHistoryBufferCaveat (US-075 criterion 3)", () => {
  it("warns about disk-vs-buffer line drift only when the document is dirty", () => {
    expect(describeLineHistoryBufferCaveat(true)).toMatch(/unsaved/i);
    expect(describeLineHistoryBufferCaveat(true)).toMatch(/disk/i);
    expect(describeLineHistoryBufferCaveat(false)).toBeUndefined();
  });
});
