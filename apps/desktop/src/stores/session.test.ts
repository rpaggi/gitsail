import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useRepositorySessionStore } from "./session";
import { useMergeStore } from "./merge";
import type { InProgressOperationDto, RepositoryDto, RepositoryStatusDto } from "../services/dto";

function repo(rootPath: string): RepositoryDto {
  return {
    id: rootPath,
    rootPath,
    worktreePath: rootPath,
    isBare: false,
    headState: { state: "attached", branch: "main" },
    currentBranch: "main",
  };
}

function status(isClean: boolean): RepositoryStatusDto {
  return {
    branch: "main",
    headState: { state: "attached", branch: "main" },
    files: [],
    isClean,
  };
}

function pendingMerge(): InProgressOperationDto {
  return {
    kind: "merge",
    heads: ["a".repeat(40)],
    conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
    capabilities: ["continue", "abort"],
  };
}

/** A promise this test can resolve/reject on its own schedule. */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("repository session store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("starts with no repository or status open, at generation 0", () => {
    const store = useRepositorySessionStore();

    expect(store.repository).toBeNull();
    expect(store.status).toBeNull();
    expect(store.generation).toBe(0);
    expect(store.isOpening).toBe(false);
    expect(store.isRefreshing).toBe(false);
  });

  it("openRepository opens the repository, refreshes its status, and reports \"opened\"", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      if (cmd === "get_repository_status") return status(true);
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = useRepositorySessionStore();
    const result = await store.openRepository("/repo");

    expect(result).toEqual({ status: "opened" });
    expect(store.repository?.rootPath).toBe("/repo");
    expect(store.status?.isClean).toBe(true);
    expect(store.isOpening).toBe(false);
    expect(store.generation).toBe(1);
  });

  it("a failed open reports \"failed\" with the error and never sets a repository", async () => {
    mockIPC(() => {
      throw { code: "repository_not_found", message: "not a Git repository" };
    });

    const store = useRepositorySessionStore();
    const result = await store.openRepository("/does-not-exist");

    expect(result).toEqual({
      status: "failed",
      error: { code: "repository_not_found", message: "not a Git repository" },
    });
    expect(store.repository).toBeNull();
    expect(store.lastError?.code).toBe("repository_not_found");
    expect(store.isOpening).toBe(false);
  });

  it(
    "US-052/US-054 isolation: a slow open for repo A superseded by opening repo B never " +
      "overwrites B once A's response finally arrives",
    async () => {
      const openA = deferred<RepositoryDto>();
      mockIPC((cmd, args) => {
        if (cmd === "open_repository") {
          const path = (args as { path: string }).path;
          if (path === "/repo-a") return openA.promise;
          return repo("/repo-b");
        }
        if (cmd === "get_repository_status") return status(true);
        throw new Error(`unexpected command ${cmd}`);
      });

      const store = useRepositorySessionStore();
      const pendingA = store.openRepository("/repo-a"); // generation 1, still "in flight"
      const doneB = await store.openRepository("/repo-b"); // generation 2, resolves first

      expect(doneB).toEqual({ status: "opened" });
      expect(store.repository?.rootPath).toBe("/repo-b");

      // Now let A's artificially delayed response finally arrive.
      openA.resolve(repo("/repo-a"));
      const resultA = await pendingA;

      expect(resultA).toEqual({ status: "superseded" });
      expect(
        store.repository?.rootPath,
        "a stale open's result must never overwrite the repository the user has since switched to",
      ).toBe("/repo-b");
    },
  );

  it("refreshStatus is a no-op while no repository is open", async () => {
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return status(true);
    });

    const store = useRepositorySessionStore();
    await store.refreshStatus("manual");

    expect(calls).toBe(0);
  });

  it("refreshStatus forwards the given reason to the backend", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      return status(true);
    });
    const store = useRepositorySessionStore();
    await store.openRepository("/repo");

    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedArgs = args;
      return status(false);
    });
    await store.refreshStatus("after_mutation");

    expect(receivedArgs).toEqual({ reason: "after_mutation" });
    expect(store.status?.isClean).toBe(false);
  });

  it("refreshStatus ignores a concurrent call while one is already in flight", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      return status(true);
    });
    const store = useRepositorySessionStore();
    await store.openRepository("/repo");

    const slow = deferred<RepositoryStatusDto>();
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return slow.promise;
    });

    const first = store.refreshStatus("manual");
    const second = store.refreshStatus("manual"); // must be ignored: a refresh is already in flight

    slow.resolve(status(false));
    await Promise.all([first, second]);

    expect(calls).toBe(1);
  });

  it(
    "US-054 criterion 3: an artificially delayed status refresh for the repository that was " +
      "open before a switch must never overwrite the status of the repository now open",
    async () => {
      mockIPC((cmd) => {
        if (cmd === "open_repository") return repo("/repo-a");
        return status(true);
      });
      const store = useRepositorySessionStore();
      await store.openRepository("/repo-a");
      const staleGeneration = store.generation;

      // A manual/focus refresh for repo-a starts, and is artificially
      // delayed (simulating a slow `git status`)...
      const slowStatus = deferred<RepositoryStatusDto>();
      mockIPC(() => slowStatus.promise);
      const stalePromise = store.refreshStatus("focus", staleGeneration);

      // ...but before it resolves, the user switches to repo-b.
      mockIPC((cmd) => {
        if (cmd === "open_repository") return repo("/repo-b");
        return status(false);
      });
      await store.openRepository("/repo-b");
      expect(store.repository?.rootPath).toBe("/repo-b");
      expect(store.status?.isClean).toBe(false);

      // Only now does repo-a's delayed status arrive.
      slowStatus.resolve(status(true));
      await stalePromise;

      expect(store.repository?.rootPath).toBe("/repo-b");
      expect(
        store.status?.isClean,
        "a stale refresh for the old repository must never overwrite the new repository's status",
      ).toBe(false);
    },
  );

  // -- T-234/US-082: re-detect the in-progress operation at the right times --

  it(
    "openRepository re-detects the in-progress operation for the newly opened repository " +
      "(criterion 2: restart/reopen reconstructs state from real Git, never from memory)",
    async () => {
      mockIPC((cmd) => {
        if (cmd === "open_repository") return repo("/repo");
        if (cmd === "get_repository_status") return status(true);
        if (cmd === "detect_in_progress_operation") return pendingMerge();
        throw new Error(`unexpected command ${cmd}`);
      });

      const session = useRepositorySessionStore();
      const merge = useMergeStore();
      // Simulates the merge/rebase/cherry-pick UI having *not* independently
      // detected anything yet — e.g. its own component hasn't mounted since
      // the app started, or this is simply a stale in-memory leftover from
      // whatever repository was open before.
      expect(merge.inProgressOperation.kind).toBe("none");

      const result = await session.openRepository("/repo");

      expect(result).toEqual({ status: "opened" });
      expect(merge.inProgressOperation.kind).toBe("merge");
      expect(merge.hasConflicts).toBe(true);
      expect(merge.supportsContinue).toBe(true);
      expect(merge.supportsSkip).toBe(false);
    },
  );

  it(
    "refreshStatus(\"focus\") re-detects the in-progress operation, but " +
      "refreshStatus(\"manual\") does not (T-234/US-082 criterion 2: regaining focus, " +
      "not every refresh, is what must pick up a change made from another terminal)",
    async () => {
      mockIPC((cmd) => {
        if (cmd === "open_repository") return repo("/repo");
        if (cmd === "get_repository_status") return status(true);
        if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
        throw new Error(`unexpected command ${cmd}`);
      });
      const session = useRepositorySessionStore();
      const merge = useMergeStore();
      await session.openRepository("/repo"); // consumes the initial detect call above

      let detectCalls = 0;
      mockIPC((cmd) => {
        if (cmd === "get_repository_status") return status(true);
        if (cmd === "detect_in_progress_operation") {
          detectCalls += 1;
          return pendingMerge();
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      await session.refreshStatus("manual");
      expect(detectCalls).toBe(0);
      expect(merge.inProgressOperation.kind).toBe("none");

      await session.refreshStatus("focus");
      expect(detectCalls).toBe(1);
      expect(merge.inProgressOperation.kind).toBe("merge");
    },
  );
});
