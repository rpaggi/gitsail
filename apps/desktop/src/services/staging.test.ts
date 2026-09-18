import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { createCommit, stageHunks, stagePaths, unstageHunks, unstagePaths } from "./staging";
import type { FileDiffDto } from "./dto";

function sampleSelection(): FileDiffDto[] {
  return [
    {
      path: "a.txt",
      previousPath: null,
      changeType: "modified",
      isBinary: false,
      truncated: false,
      hunks: [
        {
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          lines: [{ origin: "addition", content: "new", hasTrailingNewline: true }],
        },
      ],
    },
  ];
}

describe("staging service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("stagePaths invokes stage_paths with exactly the given paths", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await stagePaths(["a.txt", "b.txt"]);

    expect(receivedCommand).toBe("stage_paths");
    expect(receivedArgs).toEqual({ paths: ["a.txt", "b.txt"] });
  });

  it("unstagePaths invokes unstage_paths", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return null;
    });

    await unstagePaths(["a.txt"]);

    expect(receivedCommand).toBe("unstage_paths");
  });

  it("stageHunks echoes back exactly the given hunk selection", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const selection = sampleSelection();
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await stageHunks(selection);

    expect(receivedCommand).toBe("stage_hunks");
    expect(receivedArgs).toEqual({ selection });
  });

  it("unstageHunks invokes unstage_hunks", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return null;
    });

    await unstageHunks(sampleSelection());

    expect(receivedCommand).toBe("unstage_hunks");
  });

  it("createCommit invokes create_commit with the message and returns the new hash", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return { hash: "c".repeat(40) };
    });

    const result = await createCommit("a message");

    expect(receivedCommand).toBe("create_commit");
    expect(receivedArgs).toEqual({ message: "a message" });
    expect(result.hash).toBe("c".repeat(40));
  });
});
