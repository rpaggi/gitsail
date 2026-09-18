import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useSearchStore } from "./search";
import { useCommitGraphStore } from "./graph";
import type { BranchDto, CommitDto } from "../services/dto";

function commit(hash: string, subject: string): CommitDto {
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

function branch(name: string): BranchDto {
  return {
    name,
    kind: { kind: "local" },
    target: "a".repeat(40),
    upstream: null,
    ahead: 0,
    behind: 0,
    isCurrent: false,
  };
}

describe("search store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("a blank query clears results without issuing any request", async () => {
    let called = false;
    mockIPC(() => {
      called = true;
      return [];
    });
    const store = useSearchStore();

    await store.search("   ");

    expect(called).toBe(false);
    expect(store.commitResults).toEqual([]);
  });

  it("a text query searches commits and matching branch names", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_commits") return [commit("a".repeat(40), "fix the login bug")];
      if (cmd === "list_branches") return [branch("login-fix"), branch("main")];
      return null;
    });
    const store = useSearchStore();

    await store.search("login");

    expect(store.commitResults).toHaveLength(1);
    expect(store.branchResults.map((b) => b.name)).toEqual(["login-fix"]);
    expect(store.exactMatch).toBeNull();
  });

  it("a hash-shaped query also resolves an exact commit match", async () => {
    const hash = "b".repeat(40);
    mockIPC((cmd) => {
      if (cmd === "search_commits") return [];
      if (cmd === "list_branches") return [];
      if (cmd === "get_commit") return commit(hash, "the exact commit");
      return null;
    });
    const store = useSearchStore();

    await store.search(hash);

    expect(store.exactMatch?.subject).toBe("the exact commit");
  });

  it("a hash-shaped query that does not resolve leaves exactMatch null instead of failing the whole search", async () => {
    const hash = "c".repeat(40);
    mockIPC((cmd) => {
      if (cmd === "search_commits") return [];
      if (cmd === "list_branches") return [];
      if (cmd === "get_commit") throw { code: "repository_not_found", message: "no such commit" };
      return null;
    });
    const store = useSearchStore();

    await store.search(hash);

    expect(store.exactMatch).toBeNull();
    expect(store.lastError).toBeNull();
  });

  it("selectCommit shares identity with the commit graph store", async () => {
    const store = useSearchStore();
    const graph = useCommitGraphStore();

    store.selectCommit("d".repeat(40));

    expect(graph.selectedHash).toBe("d".repeat(40));
  });

  it(
    "T-255/US-122 criterion 3 (\"conteúdo malicioso\"): a query and matching branch names " +
      "containing control characters, ANSI escapes, or an extremely long run of text neither " +
      "crash the search nor get silently dropped",
    async () => {
      const hostileSubject = "evil[31mred " + "x".repeat(5000);
      const hostileBranch = "feature/[31m-" + "y".repeat(2000);
      mockIPC((cmd) => {
        if (cmd === "search_commits") return [commit("a".repeat(40), hostileSubject)];
        if (cmd === "list_branches") return [branch(hostileBranch), branch("main")];
        return null;
      });
      const store = useSearchStore();

      await store.search("[31m");

      expect(store.lastError).toBeNull();
      expect(store.commitResults).toHaveLength(1);
      expect(store.commitResults[0].subject).toBe(hostileSubject);
      // `branchResults` is a plain case-insensitive substring filter
      // (`.includes`), never a regex — a query containing regex
      // metacharacters (`[`, `]`) must still match literally rather than
      // being interpreted as a pattern or throwing.
      expect(store.branchResults.map((b) => b.name)).toEqual([hostileBranch]);
    },
  );

  it("clear resets query and every result bucket", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_commits") return [commit("a".repeat(40), "x")];
      if (cmd === "list_branches") return [branch("main")];
      return null;
    });
    const store = useSearchStore();
    await store.search("x");

    store.clear();

    expect(store.query).toBe("");
    expect(store.commitResults).toEqual([]);
    expect(store.branchResults).toEqual([]);
  });
});
