import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useBlameStore } from "./blame";
import type { BlameDto, CommitDto } from "../services/dto";

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
      {
        finalLine: 2,
        originalLine: 2,
        commit: "0".repeat(40),
        author: { name: "", email: "" },
        timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
        content: "uncommitted line",
        origin: "local",
      },
    ],
  };
}

function sampleCommit(hash: string): CommitDto {
  return {
    hash,
    shortHash: hash.slice(0, 8),
    parents: [],
    author: { name: "Ada", email: "ada@example.com" },
    committer: { name: "Ada", email: "ada@example.com" },
    authorDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    subject: "Add README",
    body: "",
    decorations: [],
    isMerge: false,
    isRoot: true,
  };
}

describe("blame store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("open loads blame for the given path and clears any previously selected commit", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "get_blame") {
        receivedArgs = args;
        return sampleBlame();
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBlameStore();

    await store.open("README.md");

    expect(receivedArgs).toEqual({ path: "README.md", revision: null });
    expect(store.file).toBe("README.md");
    expect(store.blame?.lines).toHaveLength(2);
    expect(store.lastError).toBeNull();
    expect(store.selectedCommit).toBeNull();
  });

  it("open records a failure without throwing", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "blame failed" };
    });
    const store = useBlameStore();

    await store.open("README.md");

    expect(store.blame).toBeNull();
    expect(store.lastError?.message).toBe("blame failed");
  });

  it("a superseded open() never overwrites the newer result", async () => {
    const store = useBlameStore();
    let resolveFirst: () => void = () => {};
    mockIPC((cmd, args) => {
      if (cmd !== "get_blame") throw new Error(`unexpected command ${cmd}`);
      const path = (args as { path: string }).path;
      if (path === "first.txt") {
        return new Promise((resolve) => {
          resolveFirst = () => resolve({ ...sampleBlame(), file: "first.txt" });
        });
      }
      return { ...sampleBlame(), file: "second.txt" };
    });

    const firstOpen = store.open("first.txt");
    await store.open("second.txt");
    resolveFirst();
    await firstOpen;

    expect(store.file).toBe("second.txt");
    expect(store.blame?.file).toBe("second.txt");
  });

  it("close resets every field, including a previously selected commit", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_blame") return sampleBlame();
      if (cmd === "get_commit") return sampleCommit("a".repeat(40));
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBlameStore();
    await store.open("README.md");
    await store.openCommitDetails("a".repeat(40));
    expect(store.selectedCommit).not.toBeNull();

    store.close();

    expect(store.file).toBeNull();
    expect(store.blame).toBeNull();
    expect(store.selectedCommit).toBeNull();
    expect(store.lastError).toBeNull();
  });

  it("openCommitDetails fetches and shows the commit for a committed line's hash", async () => {
    const hash = "a".repeat(40);
    mockIPC((cmd, args) => {
      if (cmd === "get_blame") return sampleBlame();
      if (cmd === "get_commit") {
        expect(args).toEqual({ hash });
        return sampleCommit(hash);
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBlameStore();
    await store.open("README.md");

    await store.openCommitDetails(hash);

    expect(store.selectedCommit?.hash).toBe(hash);
    expect(store.commitError).toBeNull();
  });

  it("openCommitDetails records a failure without throwing", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_blame") return sampleBlame();
      if (cmd === "get_commit") throw { code: "repository_not_found", message: "no such commit" };
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBlameStore();
    await store.open("README.md");

    await store.openCommitDetails("a".repeat(40));

    expect(store.selectedCommit).toBeNull();
    expect(store.commitError?.message).toBe("no such commit");
  });

  it("dismissCommitDetails clears the selected commit without touching the blame result", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_blame") return sampleBlame();
      if (cmd === "get_commit") return sampleCommit("a".repeat(40));
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBlameStore();
    await store.open("README.md");
    await store.openCommitDetails("a".repeat(40));

    store.dismissCommitDetails();

    expect(store.selectedCommit).toBeNull();
    expect(store.blame).not.toBeNull();
  });
});
