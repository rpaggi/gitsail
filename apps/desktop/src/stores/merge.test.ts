import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { capabilitiesOf, conflictedFilesOf, useMergeStore } from "./merge";
import { useOperationStore } from "./operation";
import type {
  CherryPickResultDto,
  ConflictSidesDto,
  InProgressOperationDto,
  MergeResultDto,
  RebasePlanDto,
  RebaseResultDto,
  RevertResultDto,
} from "../services/dto";

function samplePlan(): RebasePlanDto {
  return {
    ontoRevision: "main",
    onto: "a".repeat(40),
    branchHead: "b".repeat(40),
    entries: [
      {
        commit: "c".repeat(40),
        shortHash: "ccccccc",
        subject: "feature A",
        action: "pick",
        messageOverride: null,
      },
      {
        commit: "d".repeat(40),
        shortHash: "ddddddd",
        subject: "feature B",
        action: "pick",
        messageOverride: null,
      },
    ],
  };
}

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

  it("requestCherryPick is Moderate risk, names the exact commit, and reports Applied distinctly", async () => {
    const received: Record<string, unknown>[] = [];
    const outcome: CherryPickResultDto = { outcome: "applied", hash: "b".repeat(40) };
    mockIPC((cmd, args) => {
      if (cmd === "cherry_pick") {
        received.push(args as Record<string, unknown>);
        return outcome;
      }
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestCherryPick("a".repeat(40), "aaaaaaa", false);
    const operation = useOperationStore();
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toBe("cherry-picking commit 'aaaaaaa' onto the current branch");

    await operation.confirm();

    expect(received).toEqual([{ commit: "a".repeat(40), mergeParent: null }]);
    expect(store.lastCherryPickResult).toEqual(outcome);
    expect(operation.status).toBe("succeeded");
  });

  it("requestCherryPick against a merge commit names the first-parent policy explicitly and sends it", async () => {
    const received: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "cherry_pick") {
        received.push(args as Record<string, unknown>);
        return { outcome: "empty" } satisfies CherryPickResultDto;
      }
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestCherryPick("a".repeat(40), "aaaaaaa", true);
    expect(useOperationStore().current?.targetLabel).toContain("first parent");

    await useOperationStore().confirm();

    expect(received).toEqual([{ commit: "a".repeat(40), mergeParent: "firstParent" }]);
    expect(store.lastCherryPickResult).toEqual({ outcome: "empty" });
  });

  it("a conflicting cherry-pick never presents as a plain success — the conflict outcome is recorded distinctly", async () => {
    const conflictOutcome: CherryPickResultDto = {
      outcome: "conflict",
      conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
    };
    mockIPC((cmd) => {
      if (cmd === "cherry_pick") return conflictOutcome;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") {
        return {
          kind: "cherryPick",
          target: "a".repeat(40),
          conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
          capabilities: ["continue", "skip", "abort"],
        } satisfies InProgressOperationDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestCherryPick("a".repeat(40), "aaaaaaa", false);
    await useOperationStore().confirm();

    expect(store.lastCherryPickResult).toEqual(conflictOutcome);
    expect(store.hasConflicts).toBe(true);
  });

  it("requestRevert is Moderate risk, names the exact commit, and reports Applied distinctly", async () => {
    const received: Record<string, unknown>[] = [];
    const outcome: RevertResultDto = { outcome: "applied", hash: "c".repeat(40) };
    mockIPC((cmd, args) => {
      if (cmd === "revert") {
        received.push(args as Record<string, unknown>);
        return outcome;
      }
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestRevert("d".repeat(40), "ddddddd", false);
    const operation = useOperationStore();
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toBe("reverting commit 'ddddddd'");

    await operation.confirm();

    expect(received).toEqual([{ commit: "d".repeat(40), mergeParent: null }]);
    expect(store.lastRevertResult).toEqual(outcome);
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

  // -- EPIC-17/T-235: rebase, skip --------------------------------------

  it("requestRebase is Moderate risk, waits for confirmation, and reports completion distinctly", async () => {
    const received: string[] = [];
    const outcome: RebaseResultDto = { outcome: "completed", newHead: "c".repeat(40) };
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "rebase") return outcome;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestRebase("main");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toBe("rebasing the current branch onto 'main'");

    await operation.confirm();

    expect(received).toContain("rebase");
    expect(operation.status).toBe("succeeded");
    expect(store.lastRebaseResult).toEqual(outcome);
    expect(store.inProgressOperation).toEqual({ kind: "none" });
  });

  it("a conflicting rebase never presents as a plain success — the conflict outcome is recorded distinctly", async () => {
    const conflictOutcome: RebaseResultDto = {
      outcome: "conflict",
      conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
    };
    mockIPC((cmd) => {
      if (cmd === "rebase") return conflictOutcome;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") {
        return {
          kind: "rebase",
          interactive: false,
          onto: "a".repeat(40),
          conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
          capabilities: ["continue", "skip", "abort"],
        } satisfies InProgressOperationDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestRebase("main");
    await useOperationStore().confirm();

    expect(store.lastRebaseResult).toEqual(conflictOutcome);
    expect(store.hasConflicts).toBe(true);
    expect(store.supportsSkip).toBe(true);
    expect(useOperationStore().status).toBe("succeeded");
  });

  it("requestSkip is a no-op when the detected operation does not support it (a pending merge)", async () => {
    mockIPC((cmd) => {
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.inProgressOperation = pendingMerge(false);

    await store.requestSkip();

    expect(useOperationStore().status).toBe("idle");
  });

  it("requestSkip begins confirmation when supported, dispatches, and reinspects afterward", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "skip_operation") return null;
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.inProgressOperation = {
      kind: "rebase",
      interactive: false,
      onto: "a".repeat(40),
      conflictedFiles: [{ path: "f.txt", stage: "bothModified" }],
      capabilities: ["continue", "skip", "abort"],
    };

    await store.requestSkip();
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");

    await operation.confirm();

    expect(received).toContain("skip_operation");
    expect(operation.status).toBe("succeeded");
    expect(store.inProgressOperation).toEqual({ kind: "none" });
  });

  // -- T-236/US-084: plan an interactive rebase --------------------------

  it("requestRebasePlan loads the plan directly, without going through useOperationStore", async () => {
    const plan = samplePlan();
    mockIPC((cmd, args) => {
      if (cmd === "plan_rebase") {
        expect(args).toEqual({ ontoRevision: "main" });
        return plan;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();

    await store.requestRebasePlan("main");

    expect(store.rebasePlan).toEqual(plan);
    expect(store.rebasePlanError).toBeNull();
    expect(store.isLoadingRebasePlan).toBe(false);
    expect(useOperationStore().status).toBe("idle");
  });

  it("requestRebasePlan records a clear error without throwing, and never leaves a stale plan behind", async () => {
    mockIPC(() => {
      throw { code: "invalid_repository_state", message: "no candidates" };
    });
    const store = useMergeStore();
    store.rebasePlan = samplePlan();

    await store.requestRebasePlan("main");

    expect(store.rebasePlan).toBeNull();
    expect(store.rebasePlanError?.code).toBe("invalid_repository_state");
  });

  it("moveRebasePlanEntry reorders entries and is a no-op at either edge", () => {
    const store = useMergeStore();
    store.rebasePlan = samplePlan();

    store.moveRebasePlanEntry(1, "up");
    expect(store.rebasePlan?.entries.map((e) => e.subject)).toEqual(["feature B", "feature A"]);

    // Moving the first entry up must be a no-op.
    store.moveRebasePlanEntry(0, "up");
    expect(store.rebasePlan?.entries.map((e) => e.subject)).toEqual(["feature B", "feature A"]);

    // Moving the last entry down must be a no-op.
    store.moveRebasePlanEntry(1, "down");
    expect(store.rebasePlan?.entries.map((e) => e.subject)).toEqual(["feature B", "feature A"]);
  });

  it("setRebasePlanAction reassigns the entry's action and clears messageOverride when leaving reword", () => {
    const store = useMergeStore();
    store.rebasePlan = samplePlan();

    store.setRebasePlanAction(0, "reword");
    store.setRebasePlanMessage(0, "a better message");
    expect(store.rebasePlan?.entries[0].action).toBe("reword");
    expect(store.rebasePlan?.entries[0].messageOverride).toBe("a better message");

    store.setRebasePlanAction(0, "drop");
    expect(store.rebasePlan?.entries[0].action).toBe("drop");
    // Leaving reword for any other action must clear messageOverride.
    expect(store.rebasePlan?.entries[0].messageOverride).toBeNull();
  });

  it("validateRebasePlan mirrors RebasePlan::validate's own rules", () => {
    const store = useMergeStore();
    store.rebasePlan = samplePlan();
    expect(store.validateRebasePlan()).toBeNull();

    store.setRebasePlanAction(0, "squash");
    expect(store.validateRebasePlan()).toContain("squash");

    store.setRebasePlanAction(0, "reword");
    expect(store.validateRebasePlan()).toContain("reword entry requires");

    store.setRebasePlanMessage(0, "  ");
    expect(store.validateRebasePlan()).toContain("reword entry requires");

    store.setRebasePlanMessage(0, "a real message");
    expect(store.validateRebasePlan()).toBeNull();
  });

  it("requestExecuteRebasePlan refuses an invalid plan before ever calling execute_rebase_plan", async () => {
    mockIPC((cmd) => {
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.rebasePlan = samplePlan();
    store.setRebasePlanAction(0, "squash");

    await store.requestExecuteRebasePlan();

    expect(store.rebasePlanError?.message).toContain("squash");
    expect(useOperationStore().status).toBe("idle");
    expect(store.rebasePlan).not.toBeNull();
  });

  it("requestExecuteRebasePlan is Moderate risk, waits for confirmation, names the concrete scope, and clears the plan once dispatched", async () => {
    const received: string[] = [];
    const outcome: RebaseResultDto = { outcome: "completed", newHead: "e".repeat(40) };
    let executedPlan: RebasePlanDto | undefined;
    mockIPC((cmd, args) => {
      received.push(cmd);
      if (cmd === "execute_rebase_plan") {
        executedPlan = (args as { plan: RebasePlanDto }).plan;
        return outcome;
      }
      if (cmd === "get_repository_status") return statusResponse();
      if (cmd === "detect_in_progress_operation") return { kind: "none" } satisfies InProgressOperationDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.rebasePlan = samplePlan();

    await store.requestExecuteRebasePlan();
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toBe("rebasing 2 commit(s) onto 'main' (interactive plan)");
    // The plan is still visible while only confirmation is pending — it is
    // cleared once the mutation actually dispatches, not before.
    expect(store.rebasePlan).not.toBeNull();

    await operation.confirm();

    expect(received).toContain("execute_rebase_plan");
    expect(executedPlan).toEqual(samplePlan());
    expect(operation.status).toBe("succeeded");
    expect(store.lastRebaseResult).toEqual(outcome);
    expect(store.rebasePlan).toBeNull();
  });

  it("a stale plan's execution failure clears the plan overlay rather than silently rebuilding it", async () => {
    mockIPC((cmd) => {
      if (cmd === "execute_rebase_plan") {
        throw { code: "operation_conflict", message: "onto has moved" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useMergeStore();
    store.rebasePlan = samplePlan();

    await store.requestExecuteRebasePlan();
    await useOperationStore().confirm();

    expect(useOperationStore().status).toBe("failed");
    expect(useOperationStore().error?.code).toBe("operation_conflict");
    // A failed execution must never leave a stale plan around to retry
    // blindly.
    expect(store.rebasePlan).toBeNull();
  });

  it("closeRebasePlan discards the plan and any pending error without executing anything", () => {
    const store = useMergeStore();
    store.rebasePlan = samplePlan();
    store.rebasePlanError = { code: "internal", message: "x" };

    store.closeRebasePlan();

    expect(store.rebasePlan).toBeNull();
    expect(store.rebasePlanError).toBeNull();
  });
});
