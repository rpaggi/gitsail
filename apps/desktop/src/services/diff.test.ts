import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { getDiff } from "./diff";
import type { DiffDto } from "./dto";

function sampleDiff(): DiffDto {
  return {
    files: [
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
            lines: [
              { origin: "deletion", content: "old", hasTrailingNewline: true },
              { origin: "addition", content: "new", hasTrailingNewline: true },
            ],
          },
        ],
      },
    ],
  };
}

describe("diff service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("getDiff invokes get_diff with staged and an omitted path as null", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return sampleDiff();
    });

    const result = await getDiff({ staged: true });

    expect(receivedCommand).toBe("get_diff");
    expect(receivedArgs).toEqual({ staged: true, path: null });
    expect(result.files).toHaveLength(1);
  });

  it("getDiff forwards an explicit path filter", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return sampleDiff();
    });

    await getDiff({ staged: false, path: "a.txt" });

    expect(receivedArgs).toEqual({ staged: false, path: "a.txt" });
  });
});
