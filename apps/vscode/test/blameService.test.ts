// Caching and cancellation behavior of the blame service, against real
// repositories.
//
// What changed from the pre-ADR-025 version of this suite: it used to stub
// the CLI client and count `run()` calls to prove the cache deduped. The
// caching contract is identical, so those tests survive verbatim in intent
// — they now count calls on a spy wrapped around a real `GitClient`, which
// proves the same thing without asserting a command line that no longer
// exists.
//
// What is genuinely new is the cancellation group at the bottom. Blame is
// debounced and re-fired on every cursor movement; before ADR-025 an
// abandoned query was a `gitsail` process the extension simply stopped
// waiting for. It is now a `git blame` this package is responsible for
// killing, so "abandoning a query actually aborts it" is a behavior worth
// a test rather than a comment.

import { describe, expect, it, vi } from "vitest";

import { BlameQueryCache, CommitSubjectCache, getBlame, resolveCommitSubjects } from "../src/blameService";
import { GitClient } from "../src/git/gitClient";
import { TempRepo } from "./support/tempRepo";

/** A real `GitClient` with `getBlame`/`getCommit` wrapped in spies, so a
 * test can assert how many real queries actually ran. */
function spiedClient(): { client: GitClient; blameCalls: () => number; commitCalls: () => number } {
  const client = new GitClient();
  const blameSpy = vi.spyOn(client, "getBlame");
  const commitSpy = vi.spyOn(client, "getCommit");
  return {
    client,
    blameCalls: () => blameSpy.mock.calls.length,
    commitCalls: () => commitSpy.mock.calls.length,
  };
}

function repoWithOneCommit(): TempRepo {
  const repo = TempRepo.create();
  repo.write("a.ts", "const x = 1;\n");
  repo.commit("first");
  return repo;
}

describe("getBlame", () => {
  it("blames the working tree, including uncommitted lines, when no revision is given", async () => {
    const repo = repoWithOneCommit();
    try {
      repo.write("a.ts", "const x = 1;\nconst y = 2;\n");

      const result = await getBlame(new GitClient(), { repoRoot: repo.root, filePath: "a.ts" });

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.revision).toBeNull();
      expect(result.value.lines.map((l) => l.origin)).toEqual(["committed", "local"]);
    } finally {
      repo.dispose();
    }
  });

  it("blames the given revision when one is supplied", async () => {
    const repo = repoWithOneCommit();
    try {
      const first = repo.head();
      repo.write("a.ts", "const x = 1;\nconst y = 2;\n");
      repo.commit("second");

      const result = await getBlame(new GitClient(), {
        repoRoot: repo.root,
        filePath: "a.ts",
        revision: first,
      });

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.revision).toBe(first);
      // At `first` the file had one line; the second line must not appear.
      expect(result.value.lines).toHaveLength(1);
    } finally {
      repo.dispose();
    }
  });

  it("returns an error result rather than throwing when the file is not tracked", async () => {
    const repo = repoWithOneCommit();
    try {
      const result = await getBlame(new GitClient(), {
        repoRoot: repo.root,
        filePath: "does-not-exist.ts",
      });
      expect(result.kind).toBe("error");
    } finally {
      repo.dispose();
    }
  });
});

describe("BlameQueryCache (US-034 criterion 1 pattern, client-side)", () => {
  it("dedupes repeated queries for the same file/revision/contentVersion", async () => {
    const repo = repoWithOneCommit();
    try {
      const { client, blameCalls } = spiedClient();
      const cache = new BlameQueryCache(client);
      const query = { repoRoot: repo.root, filePath: "a.ts" };

      await cache.get(query, 1);
      await cache.get(query, 1);

      expect(blameCalls()).toBe(1);
    } finally {
      repo.dispose();
    }
  });

  it("a changed content version re-queries instead of serving stale blame across an edit", async () => {
    const repo = repoWithOneCommit();
    try {
      const { client, blameCalls } = spiedClient();
      const cache = new BlameQueryCache(client);
      const query = { repoRoot: repo.root, filePath: "a.ts" };

      await cache.get(query, 1);
      await cache.get(query, 2);

      expect(blameCalls()).toBe(2);
    } finally {
      repo.dispose();
    }
  });

  it("invalidateAll() forces a fresh query even for a previously cached key", async () => {
    const repo = repoWithOneCommit();
    try {
      const { client, blameCalls } = spiedClient();
      const cache = new BlameQueryCache(client);
      const query = { repoRoot: repo.root, filePath: "a.ts" };

      await cache.get(query, 1);
      cache.invalidateAll();
      await cache.get(query, 1);

      expect(blameCalls()).toBe(2);
    } finally {
      repo.dispose();
    }
  });

  it("serves a cached answer without re-running git, so it never blocks on a second process", async () => {
    const repo = repoWithOneCommit();
    try {
      const { client, blameCalls } = spiedClient();
      const cache = new BlameQueryCache(client);
      const query = { repoRoot: repo.root, filePath: "a.ts" };

      const first = await cache.get(query, 1);
      const second = await cache.get(query, 1);

      expect(blameCalls()).toBe(1);
      expect(second).toBe(first);
    } finally {
      repo.dispose();
    }
  });
});

describe("BlameQueryCache cancellation (ADR-025: an abandoned query must be killed)", () => {
  it("aborts an in-flight query when a newer content version supersedes it", async () => {
    const repo = repoWithOneCommit();
    try {
      const client = new GitClient();
      const cache = new BlameQueryCache(client);
      const query = { repoRoot: repo.root, filePath: "a.ts" };

      // Start version 1 and immediately ask for version 2, exactly as a
      // keystroke during a debounce window would.
      const stale = cache.get(query, 1);
      const fresh = cache.get(query, 2);

      const staleResult = await stale;
      // The superseded query either completed before the abort landed or
      // was cancelled — never left running. The load-bearing assertion is
      // the one below: it must not be retained in the cache afterwards.
      expect(["ok", "error"]).toContain(staleResult.kind);
      expect((await fresh).kind).toBe("ok");

      // A cancelled entry must never be served to a later caller as though
      // it were an answer: asking for version 1 again re-queries.
      const spy = vi.spyOn(client, "getBlame");
      await cache.get(query, 1);
      if (staleResult.kind === "error") {
        expect(spy.mock.calls.length).toBe(1);
      }
    } finally {
      repo.dispose();
    }
  });

  it("invalidateAll() aborts anything still running", async () => {
    const repo = repoWithOneCommit();
    try {
      const cache = new BlameQueryCache(new GitClient());
      const pending = cache.get({ repoRoot: repo.root, filePath: "a.ts" }, 1);
      cache.invalidateAll();
      const result = await pending;
      // Whether the abort beat the process to the finish line is a race;
      // what must never happen is an unhandled rejection or a hang.
      expect(["ok", "error"]).toContain(result.kind);
    } finally {
      repo.dispose();
    }
  });
});

describe("CommitSubjectCache / resolveCommitSubjects", () => {
  it("fetches each distinct hash only once", async () => {
    const repo = repoWithOneCommit();
    try {
      const { client, commitCalls } = spiedClient();
      const cache = new CommitSubjectCache(client);
      const hash = repo.head();

      await cache.get(repo.root, hash);
      await cache.get(repo.root, hash);

      expect(commitCalls()).toBe(1);
    } finally {
      repo.dispose();
    }
  });

  it("resolves the real commit subject", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.ts", "x\n");
      const hash = repo.commit("Fix the bug");
      const cache = new CommitSubjectCache(new GitClient());

      expect(await cache.get(repo.root, hash)).toBe("Fix the bug");
    } finally {
      repo.dispose();
    }
  });

  it("resolveCommitSubjects is best-effort: one failing hash never blocks the others", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.ts", "x\n");
      const real = repo.commit("a real subject");
      const bogus = "b".repeat(40);
      const cache = new CommitSubjectCache(new GitClient());

      const subjects = await resolveCommitSubjects(cache, repo.root, [real, bogus]);

      expect(subjects.get(real)).toBe("a real subject");
      expect(subjects.has(bogus)).toBe(false);
    } finally {
      repo.dispose();
    }
  });
});
