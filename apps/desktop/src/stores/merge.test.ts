import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { capabilitiesOf, conflictedFilesOf, useMergeStore } from "./merge";
import { useOperationStore } from "./operation";
import type { ConflictSidesDto, InProgressOperationDto, MergeResultDto } from "../services/dto";

function statusResponse() {
  return { branch: "main", headState: { state: "attached", branch: "main" }, files: [], isClean: true };
}

function pendingMerge(conflicted: boolean): InProgressOperationDto {
  return {
    kind: "merge",
    heads: ["a".repeat(40)],
    conflictedFiles: conflicted ? [{ path: "f.txt", stage: "bothModified" }] : [],
    capabilities: ["continue", "abort"],
  };
}

describe("merge/conflicts helpers", () => {
  it("conflictedFilesOf/capabilitiesOf report empty for 'none'", () => {
    expect(conflictedFilesOf({ kind: "none" })).toEqual([]);
    expect(capabilitiesOf({ kind: "none" })).toEqual([]);
  });

  it("conflictedFilesOf/capabilitiesOf read through a pending merge", () => {
    const op = pendingMerge(true);
    expect(conflictedFilesOf(op)).toHaveLength(1);
    expect(capabilitiesOf(op)).toEqual(["continue", "abort"]);
  });
});

describe("merge store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("refreshInProgressOperation loads and records the detected state", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_in_progress_operation") return pendingMerge(true);
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.refreshInProgressOperation();

    expect(store.inProgressOperation.kind).toBe("merge");
    expect(store.hasConflicts).toBe(true);
    expect(store.supportsContinue).toBe(true);
    expect(store.supportsAbort).toBe(true);
  });

  it("requestMerge is Moderate risk, waits for confirmation, and reports a fast-forward outcome distinctly", async () => {
    const received: string[] = [];
    const outcome: MergeResultDto = { outcome: "fastForwarded", newHead: "b".repeat(40) };
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "merge") return outcome;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestMerge("feature/x");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toBe("merging 'feature/x' into the current branch");

    await operation.confirm();

    expect(received).toContain("merge");
    expect(operation.status).toBe("succeeded");
    expect(store.lastMergeResult).toEqual(outcome);
    expect(store.inProgressOperation).toEqual({ kind: "none" });
  });

  it("a conflicting merge never presents as a plain success — the conflict outcome is recorded distinctly", async () => {
    const conflictOutcome: MergeResultDto = {
      outcome: "conflict",
      conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
    };
    mockIPC((cmd) => {
      if (cmd === "merge") return conflictOutcome;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return pendingMerge(true);
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestMerge("feature/x");
    await useOperationStore().confirm();

    expect(store.lastMergeResult).toEqual(conflictOutcome);
    expect(store.hasConflicts).toBe(true);
    expect(useOperationStore().status).toBe("succeeded");
  });

  it("inspectConflict loads the base/ours/theirs sides for the given path", async () => {
    const sides: ConflictSidesDto = {
      path: "f.txt",
      base: { kind: "text", text: "base\n" },
      ours: { kind: "text", text: "ours\n" },
      theirs: { kind: "text", text: "theirs\n" },
    };
    mockIPC((cmd) => {
      if (cmd === "get_conflict_sides") return sides;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.inspectConflict("f.txt");

    expect(store.inspectedPath).toBe("f.txt");
    expect(store.inspectedSides).toEqual(sides);
    expect(store.conflictError).toBeNull();
  });

  it("inspectConflict records a clear error without throwing", async () => {
    mockIPC(() => {
      throw { code: "repository_not_found", message: "no such path" };
    });
    const store = useMergeStore();

    await store.inspectConflict("missing.txt");

    expect(store.inspectedSides).toBeNull();
    expect(store.conflictError?.code).toBe("repository_not_found");
  });

  it("markResolved is Safe risk; it dispatches without confirmation and refreshes", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "mark_conflict_resolved") return null;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.markResolved("f.txt");

    expect(received).toContain("mark_conflict_resolved");
    expect(useOperationStore().status).toBe("succeeded");
    expect(store.inProgressOperation).toEqual({ kind: "none" });
  });

  it("takeSide is Moderate risk and waits for confirmation", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "take_conflict_side") return null;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.takeSide("img.bin", "theirs");
    expect(received).toEqual([]);
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.targetLabel).toBe("'img.bin' (take theirs)");

    await operation.confirm();

    expect(received).toContain("take_conflict_side");
    expect(operation.status).toBe("succeeded");
  });

  it("requestContinue is a no-op when the detected operation does not support it", async () => {
    mockIPC((cmd) => {
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.inProgressOperation = {
      kind: "bisectRun",
      conflictedFiles: [],
      capabilities: ["skip", "abort"],
    };

    await store.requestContinue();

    expect(useOperationStore().status).toBe("idle");
  });

  it("requestContinue and requestAbort begin confirmation when supported, and reinspect afterward", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "continue_operation" || cmd === "abort_operation") return null;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.inProgressOperation = pendingMerge(false);

    await store.requestContinue();
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    await operation.confirm();
    expect(received).toContain("continue_operation");
    expect(store.inProgressOperation).toEqual({ kind: "none" });

    // Reset for abort.
    store.inProgressOperation = pendingMerge(false);
    await store.requestAbort();
    expect(operation.current?.risk).toBe("destructive");
    expect(operation.current?.impact).toBeTruthy();
    await operation.confirm();
    expect(received).toContain("abort_operation");
    expect(store.inProgressOperation).toEqual({ kind: "none" });
  });
});
