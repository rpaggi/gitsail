import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { amendCommit, previewAmend } from "./amend";
import type { AmendPreviewDto } from "./dto";

function samplePreview(): AmendPreviewDto {
  return {
    head: {
      hash: "a".repeat(40),
      shortHash: "aaaaaaaa",
      parents: [],
      author: { name: "Ada", email: "ada@example.com" },
      committer: { name: "Ada", email: "ada@example.com" },
      authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      subject: "original message",
      body: "",
      decorations: [],
      isMerge: false,
      isRoot: true,
    },
    stagedDiff: { files: [] },
  };
}

describe("amend service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("previewAmend invokes preview_amend and returns HEAD plus the staged diff", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return samplePreview();
    });

    const preview = await previewAmend();

    expect(receivedCommand).toBe("preview_amend");
    expect(preview.head.subject).toBe("original message");
  });

  it("amendCommit sends the message and the expected HEAD hash", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return { hash: "b".repeat(40) };
    });

    const result = await amendCommit("amended message", "a".repeat(40));

    expect(receivedCommand).toBe("amend_commit");
    expect(receivedArgs).toEqual({ message: "amended message", expectedHead: "a".repeat(40) });
    expect(result.hash).toBe("b".repeat(40));
  });
});
