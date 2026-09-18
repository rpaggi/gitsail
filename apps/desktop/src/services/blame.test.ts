import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { getBlame } from "./blame";
import type { BlameDto } from "./dto";

function sampleBlame(): BlameDto {
  return {
    file: "README.md",
    revision: null,
    lines: [
      {
        finalLine: 1,
        originalLine: 1,
        commit: "a".repeat(40),
        author: { name: "Ada", email: "ada@example.com" },
        timestamp: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
        content: "# README",
        origin: "committed",
      },
    ],
  };
}

describe("blame service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("getBlame sends the path and an omitted revision as null", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return sampleBlame();
    });

    const blame = await getBlame({ path: "README.md" });

    expect(receivedCommand).toBe("get_blame");
    expect(receivedArgs).toEqual({ path: "README.md", revision: null });
    expect(blame.lines).toHaveLength(1);
    expect(blame.lines[0].origin).toBe("committed");
  });

  it("getBlame forwards an explicit revision", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return sampleBlame();
    });

    await getBlame({ path: "README.md", revision: "v1.0" });

    expect(receivedArgs).toEqual({ path: "README.md", revision: "v1.0" });
  });
});
