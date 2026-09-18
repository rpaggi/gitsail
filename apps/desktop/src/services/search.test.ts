import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { getCommit, looksLikeCommitHash, searchCommits } from "./search";
import type { CommitDto } from "./dto";

function sampleCommit(hash: string, subject: string): CommitDto {
  return {
    hash,
    shortHash: hash.slice(0, 8),
    parents: [],
    author: { name: "Ada", email: "ada@example.com" },
    committer: { name: "Ada", email: "ada@example.com" },
    authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    subject,
    body: "",
    decorations: [],
    isMerge: false,
    isRoot: false,
  };
}

describe("search service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("searchCommits sends every filter, omitted ones as null", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return [sampleCommit("a".repeat(40), "fix bug")];
    });

    const results = await searchCommits({ textQuery: "fix" });

    expect(receivedCommand).toBe("search_commits");
    expect(receivedArgs).toEqual({
      textQuery: "fix",
      author: null,
      branch: null,
      revisionRange: null,
      limit: null,
    });
    expect(results).toHaveLength(1);
  });

  it("searchCommits forwards every filter when given", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return [];
    });

    await searchCommits({
      textQuery: "fix",
      author: "ada",
      branch: "main",
      revisionRange: "main..dev",
      limit: 10,
    });

    expect(receivedArgs).toEqual({
      textQuery: "fix",
      author: "ada",
      branch: "main",
      revisionRange: "main..dev",
      limit: 10,
    });
  });

  it("getCommit invokes get_commit with the given hash", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const hash = "b".repeat(40);
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return sampleCommit(hash, "a specific commit");
    });

    const commit = await getCommit(hash);

    expect(receivedCommand).toBe("get_commit");
    expect(receivedArgs).toEqual({ hash });
    expect(commit.subject).toBe("a specific commit");
  });

  it.each([
    ["a".repeat(40), true],
    ["abc1234", true],
    ["ABC1234", true],
    ["not a hash", false],
    ["abc", false],
    ["", false],
  ])("looksLikeCommitHash(%s) === %s", (input, expected) => {
    expect(looksLikeCommitHash(input)).toBe(expected);
  });
});
