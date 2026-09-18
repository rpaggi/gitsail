import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import {
  abortOperation,
  continueOperation,
  detectInProgressOperation,
  getConflictSides,
  markConflictResolved,
  merge,
  takeConflictSide,
} from "./merge";
import type { ConflictSidesDto, InProgressOperationDto, MergeResultDto } from "./dto";

describe("merge service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("detectInProgressOperation invokes detect_in_progress_operation and returns the dto", async () => {
    let receivedCommand = "";
    const operation: InProgressOperationDto = { kind: "none" };
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return operation;
    });

    const result = await detectInProgressOperation();

    expect(receivedCommand).toBe("detect_in_progress_operation");
    expect(result).toEqual(operation);
  });

  it("merge invokes the merge command with the exact target revision", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const outcome: MergeResultDto = { outcome: "fastForwarded", newHead: "a".repeat(40) };
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return outcome;
    });

    const result = await merge("feature/x");

    expect(receivedCommand).toBe("merge");
    expect(receivedArgs).toEqual({ targetRevision: "feature/x" });
    expect(result).toEqual(outcome);
  });

  it("getConflictSides invokes get_conflict_sides with the exact path", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const sides: ConflictSidesDto = {
      path: "f.txt",
      base: { kind: "text", text: "base\n" },
      ours: { kind: "text", text: "ours\n" },
      theirs: { kind: "text", text: "theirs\n" },
    };
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return sides;
    });

    const result = await getConflictSides("f.txt");

    expect(receivedCommand).toBe("get_conflict_sides");
    expect(receivedArgs).toEqual({ path: "f.txt" });
    expect(result).toEqual(sides);
  });

  it("markConflictResolved invokes mark_conflict_resolved with the exact path", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await markConflictResolved("f.txt");

    expect(receivedCommand).toBe("mark_conflict_resolved");
    expect(receivedArgs).toEqual({ path: "f.txt" });
  });

  it("takeConflictSide invokes take_conflict_side with the exact path and side", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await takeConflictSide("img.bin", "theirs");

    expect(receivedCommand).toBe("take_conflict_side");
    expect(receivedArgs).toEqual({ path: "img.bin", side: "theirs" });
  });

  it("continueOperation invokes continue_operation with no arguments", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await continueOperation();

    expect(receivedCommand).toBe("continue_operation");
    expect(receivedArgs).toEqual({});
  });

  it("abortOperation invokes abort_operation with no arguments", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await abortOperation();

    expect(receivedCommand).toBe("abort_operation");
    expect(receivedArgs).toEqual({});
  });
});
