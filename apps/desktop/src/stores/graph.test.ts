import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useCommitGraphStore } from "./graph";
import type { CommitGraphRowDto } from "../services/dto";

function row(hash: string, lane: number, resolved = true): CommitGraphRowDto {
  return {
    commit: {
      hash,
      shortHash: hash.slice(0, 8),
      parents: [],
      author: { name: "Ada", email: "ada@example.com" },
      committer: { name: "Ada", email: "ada@example.com" },
      authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      subject: `commit ${hash.slice(0, 8)}`,
      body: "",
      decorations: [],
      isMerge: false,
      isRoot: false,
    },
    lane,
    edges: [{ fromLane: lane, toLane: lane, target: "z".repeat(40), resolved }],
    passthroughLanes: [],
  };
}

describe("commit graph store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("starts empty with no rows and nothing loaded", () => {
    const store = useCommitGraphStore();
    expect(store.rows).toEqual([]);
    expect(store.hasMore).toBe(false);
    expect(store.selectedHash).toBeNull();
  });

  it("loadFirstPage replaces rows with the returned page, reset:true", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return {
        rows: [row("a".repeat(40), 0)],
        laneCount: 1,
        hasMore: true,
        nextCursor: "1",
      };
    });

    const store = useCommitGraphStore();
    await store.loadFirstPage();

    expect(receivedArgs).toMatchObject({ reset: true });
    expect(store.rows).toHaveLength(1);
    expect(store.hasMore).toBe(true);
    expect(store.nextCursor).toBe("1");
    expect(store.lastError).toBeNull();
  });

  it(
    "T-255/US-122 criterion 3 (\"repo vazio\"): loadFirstPage against a freshly initialized " +
      "repository with no commits yet returns an empty page, not an error",
    async () => {
      mockIPC(() => ({ rows: [], laneCount: 0, hasMore: false, nextCursor: null }));

      const store = useCommitGraphStore();
      await store.loadFirstPage();

      expect(store.rows).toEqual([]);
      expect(store.hasMore).toBe(false);
      expect(store.nextCursor).toBeNull();
      expect(store.isLoading).toBe(false);
      expect(store.lastError).toBeNull();
    },
  );

  it("loadMore appends to (never replaces) the already-loaded rows, reset:false", async () => {
    let callCount = 0;
    mockIPC((_cmd, args) => {
      callCount += 1;
      if (callCount === 1) {
        return { rows: [row("a".repeat(40), 0)], laneCount: 1, hasMore: true, nextCursor: "1" };
      }
      expect(args).toMatchObject({ reset: false, cursor: "1" });
      return { rows: [row("b".repeat(40), 0)], laneCount: 1, hasMore: false, nextCursor: null };
    });

    const store = useCommitGraphStore();
    await store.loadFirstPage();
    const firstRowBeforeAppend = store.rows[0];

    await store.loadMore();

    expect(store.rows).toHaveLength(2);
    expect(store.rows[0]).toBe(firstRowBeforeAppend);
    expect(store.rows[1].commit.hash).toBe("b".repeat(40));
    expect(store.hasMore).toBe(false);
  });

  it("loadMore is a no-op once hasMore is false", async () => {
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return { rows: [row("a".repeat(40), 0)], laneCount: 1, hasMore: false, nextCursor: null };
    });

    const store = useCommitGraphStore();
    await store.loadFirstPage();
    expect(calls).toBe(1);

    await store.loadMore();
    expect(calls).toBe(1);
  });

  it("a selection tracked by hash resolves to the same row after loadMore appends a page", async () => {
    let callCount = 0;
    mockIPC(() => {
      callCount += 1;
      if (callCount === 1) {
        return {
          rows: [row("a".repeat(40), 0), row("b".repeat(40), 0)],
          laneCount: 1,
          hasMore: true,
          nextCursor: "2",
        };
      }
      return { rows: [row("c".repeat(40), 0)], laneCount: 1, hasMore: false, nextCursor: null };
    });

    const store = useCommitGraphStore();
    await store.loadFirstPage();
    store.select("b".repeat(40));
    const indexBefore = store.rowIndexOf("b".repeat(40));
    expect(indexBefore).toBe(1);

    await store.loadMore();

    expect(store.rowIndexOf("b".repeat(40))).toBe(indexBefore);
    expect(store.selectedHash).toBe("b".repeat(40));
  });

  it("hover tracks a separate hash from selection", () => {
    const store = useCommitGraphStore();
    store.select("a".repeat(40));
    store.hover("b".repeat(40));

    expect(store.selectedHash).toBe("a".repeat(40));
    expect(store.hoverHash).toBe("b".repeat(40));
  });

  it("a failed load surfaces a typed error and never touches previously loaded rows", async () => {
    let callCount = 0;
    mockIPC(() => {
      callCount += 1;
      if (callCount === 1) {
        return { rows: [row("a".repeat(40), 0)], laneCount: 1, hasMore: true, nextCursor: "1" };
      }
      throw { code: "process_failure", message: "git log failed" };
    });

    const store = useCommitGraphStore();
    await store.loadFirstPage();
    await store.loadMore();

    expect(store.lastError?.code).toBe("process_failure");
    // A failed loadMore must not discard already-loaded rows.
    expect(store.rows).toHaveLength(1);
  });
});
