import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { getCommitGraphPage } from "./graph";
import type { CommitGraphPageDto } from "./dto";

function samplePage(): CommitGraphPageDto {
  return {
    rows: [
      {
        commit: {
          hash: "a".repeat(40),
          shortHash: "aaaaaaaa",
          parents: [],
          author: { name: "Ada", email: "ada@example.com" },
          committer: { name: "Ada", email: "ada@example.com" },
          authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
          commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
          subject: "initial commit",
          body: "",
          decorations: [],
          isMerge: false,
          isRoot: true,
        },
        lane: 0,
        edges: [],
        passthroughLanes: [],
      },
    ],
    laneCount: 1,
    hasMore: false,
    nextCursor: null,
  };
}

describe("commit graph service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("getCommitGraphPage invokes get_commit_graph_page with the given params", async () => {
    const page = samplePage();
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return page;
    });

    const result = await getCommitGraphPage({ branch: "main", cursor: "5", limit: 50, reset: true });

    expect(receivedCommand).toBe("get_commit_graph_page");
    expect(receivedArgs).toEqual({ branch: "main", cursor: "5", limit: 50, reset: true });
    expect(result).toEqual(page);
  });

  it("omitted optional params are sent as null, never undefined or a missing key", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return samplePage();
    });

    await getCommitGraphPage({ reset: false });

    expect(receivedArgs).toEqual({ branch: null, cursor: null, limit: null, reset: false });
  });
});
