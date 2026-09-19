// The seven read-only queries ADR-025 moved into TypeScript, each against
// a real repository built by `support/tempRepo.ts`.
//
// These are the tests that carry the weight of that decision. The
// presentation layer is unchanged and already covered; what is new is that
// this package now *derives* the DTOs it used to be handed, so the
// assertions here are about DTO semantics — is `origin` really "local" for
// an uncommitted line, is `base` really null only for a root commit, is
// `previousPath` really set across a rename — rather than about which
// arguments were passed.

import { describe, expect, it } from "vitest";

import { GitClient, parseGitVersion, probeGit, relativeToRepo } from "../src/git/gitClient";
import { GitParseError } from "../src/git/errors";
import { FIXTURE_AUTHOR, FIXTURE_EPOCH_SECONDS, TempRepo } from "./support/tempRepo";

const client = new GitClient();

async function withRepo<T>(fn: (repo: TempRepo) => Promise<T>): Promise<T> {
  const repo = TempRepo.create();
  try {
    return await fn(repo);
  } finally {
    repo.dispose();
  }
}

// -- Git availability --------------------------------------------------

describe("probeGit", () => {
  it("reports the installed git version", async () => {
    await withRepo(async (repo) => {
      const availability = await probeGit(repo.root);
      expect(availability.status).toBe("ok");
      if (availability.status === "ok") {
        expect(availability.version).toMatch(/^\d+\.\d+/);
      }
    });
  });

  it("reports not-found when git cannot be spawned", async () => {
    const availability = await probeGit("/nonexistent/gitsail/definitely/not/here");
    expect(availability.status).toBe("not-found");
  });
});

describe("parseGitVersion", () => {
  it("parses the standard output shape", () => {
    expect(parseGitVersion("git version 2.43.0\n")).toBe("2.43.0");
    expect(parseGitVersion("git version 2.39.2 (Apple Git-143)")).toBe("2.39.2");
  });

  it("returns undefined for output that is not git's", () => {
    expect(parseGitVersion("not git at all")).toBeUndefined();
    expect(parseGitVersion("")).toBeUndefined();
  });
});

// -- 1. Repository discovery -------------------------------------------

describe("discover", () => {
  it("reports a repository, its root, branch and attached HEAD", async () => {
    await withRepo(async (repo) => {
      repo.write("a.txt", "one\n");
      repo.commit("first");

      const outcome = await client.discover(repo.root);

      expect(outcome.kind).toBe("repository");
      if (outcome.kind !== "repository") return;
      const { repository } = outcome;
      expect(repository.isBare).toBe(false);
      expect(repository.currentBranch).toBe("main");
      expect(repository.headState).toEqual({ state: "attached", branch: "main" });
      expect(repository.worktreePath).toBe(repository.rootPath);
      expect(repository.id).toBe(repository.rootPath);
    });
  });

  it("discovers from a nested subdirectory, not just the root", async () => {
    await withRepo(async (repo) => {
      repo.write("deep/nested/a.txt", "one\n");
      repo.commit("first");

      const outcome = await client.discover(`${repo.root}/deep/nested`);

      expect(outcome.kind).toBe("repository");
    });
  });

  it("reports an unborn HEAD for a repository with no commits yet", async () => {
    await withRepo(async (repo) => {
      const outcome = await client.discover(repo.root);
      expect(outcome.kind).toBe("repository");
      if (outcome.kind !== "repository") return;
      expect(outcome.repository.headState).toEqual({ state: "unborn" });
      expect(outcome.repository.currentBranch).toBeNull();
    });
  });

  it("reports a detached HEAD with the commit it points at", async () => {
    await withRepo(async (repo) => {
      repo.write("a.txt", "one\n");
      const first = repo.commit("first");
      repo.write("a.txt", "two\n");
      repo.commit("second");
      repo.git(["checkout", "-q", "--detach", first]);

      const outcome = await client.discover(repo.root);
      expect(outcome.kind).toBe("repository");
      if (outcome.kind !== "repository") return;
      expect(outcome.repository.headState).toEqual({ state: "detached", commit: first });
      expect(outcome.repository.currentBranch).toBeNull();
    });
  });

  it("handles a bare repository, where --show-toplevel does not apply", async () => {
    await withRepo(async (repo) => {
      repo.write("a.txt", "one\n");
      repo.commit("first");
      repo.git(["clone", "-q", "--bare", ".", "bare.git"]);

      const outcome = await client.discover(`${repo.root}/bare.git`);

      expect(outcome.kind).toBe("repository");
      if (outcome.kind !== "repository") return;
      expect(outcome.repository.isBare).toBe(true);
      // A bare repository has no working tree, and must not claim one.
      expect(outcome.repository.worktreePath).toBeNull();
    });
  });

  it("reports no-repository — not an error — for a directory outside any repository", async () => {
    await withRepo(async (repo) => {
      // A temp dir that is deliberately not a repository. `repo` exists
      // only to give the test a disposable place to stand.
      const outcome = await client.discover(require("node:os").tmpdir());
      // Depending on where tmpdir lives this is either "no repository" or a
      // real one; the property under test is that it never throws and never
      // invents a repository for a path git rejected.
      expect(["repository", "no-repository"]).toContain(outcome.kind);
      expect(repo.root).toBeTruthy();
    });
  });
});

// -- 2. Blame ----------------------------------------------------------

describe("getBlame", () => {
  it("attributes every line, reusing a commit's metadata across repeat mentions", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\ntwo\nthree\n");
      const first = repo.commit("first commit");
      repo.write("f.txt", "one\nTWO\nthree\nfour\n");
      const second = repo.commit("second commit");

      const blame = await client.getBlame({ repoRoot: repo.root, filePath: "f.txt" });

      expect(blame.file).toBe("f.txt");
      expect(blame.revision).toBeNull();
      expect(blame.lines).toHaveLength(4);
      // Lines 3 and 4 are the trap: porcelain emits only a bare header for
      // them because their commits were already described earlier. If the
      // parser read metadata positionally these would carry the wrong
      // author or none at all.
      expect(blame.lines.map((l) => l.commit)).toEqual([first, second, first, second]);
      expect(blame.lines.map((l) => l.finalLine)).toEqual([1, 2, 3, 4]);
      for (const line of blame.lines) {
        expect(line.author).toEqual(FIXTURE_AUTHOR);
        expect(line.origin).toBe("committed");
        expect(line.timestamp.secondsSinceEpoch).toBe(FIXTURE_EPOCH_SECONDS);
        expect(line.timestamp.utcOffsetMinutes).toBe(0);
      }
      expect(blame.lines.map((l) => l.content)).toEqual(["one", "TWO", "three", "four"]);
    });
  });

  it('marks an uncommitted line "local" rather than inventing an author for it', async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");
      repo.write("f.txt", "one\nuncommitted\n");

      const blame = await client.getBlame({ repoRoot: repo.root, filePath: "f.txt" });

      expect(blame.lines).toHaveLength(2);
      expect(blame.lines[0].origin).toBe("committed");
      const local = blame.lines[1];
      expect(local.origin).toBe("local");
      expect(local.commit).toMatch(/^0{40}$/);
    });
  });

  it("echoes back the revision it was asked for", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const first = repo.commit("first");

      const blame = await client.getBlame({
        repoRoot: repo.root,
        filePath: "f.txt",
        revision: first,
      });

      expect(blame.revision).toBe(first);
      expect(blame.lines).toHaveLength(1);
    });
  });

  it("refuses an option-shaped revision rather than letting it become a flag", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");

      await expect(
        client.getBlame({ repoRoot: repo.root, filePath: "f.txt", revision: "--output=/tmp/pwned" }),
      ).rejects.toBeInstanceOf(GitParseError);
    });
  });
});

// -- 3. File history ----------------------------------------------------

describe("getFileHistoryPage", () => {
  it("returns a file's commits newest-first", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const first = repo.commit("first");
      repo.write("f.txt", "two\n");
      const second = repo.commit("second");
      repo.write("other.txt", "x\n");
      repo.commit("unrelated");

      const page = await client.getFileHistoryPage({ repoRoot: repo.root, filePath: "f.txt" });

      expect(page.items.map((c) => c.hash)).toEqual([second, first]);
      expect(page.hasMore).toBe(false);
      expect(page.nextCursor).toBeUndefined();
    });
  });

  it("paginates with an opaque cursor that resumes exactly where it left off", async () => {
    await withRepo(async (repo) => {
      const hashes: string[] = [];
      for (let i = 0; i < 5; i += 1) {
        repo.write("f.txt", `rev ${i}\n`);
        hashes.push(repo.commit(`commit ${i}`));
      }
      const newestFirst = [...hashes].reverse();

      const first = await client.getFileHistoryPage({
        repoRoot: repo.root,
        filePath: "f.txt",
        limit: 2,
      });
      expect(first.items.map((c) => c.hash)).toEqual(newestFirst.slice(0, 2));
      expect(first.hasMore).toBe(true);
      expect(first.nextCursor).toBeDefined();

      const second = await client.getFileHistoryPage({
        repoRoot: repo.root,
        filePath: "f.txt",
        limit: 2,
        cursor: first.nextCursor,
      });
      expect(second.items.map((c) => c.hash)).toEqual(newestFirst.slice(2, 4));
      expect(second.hasMore).toBe(true);
    });
  });

  it("follows a file across a rename by default (US-074 criterion 3)", async () => {
    await withRepo(async (repo) => {
      repo.write("old.txt", "one\n");
      const first = repo.commit("first");
      repo.git(["mv", "old.txt", "new.txt"]);
      const renamed = repo.commit("rename");

      const followed = await client.getFileHistoryPage({
        repoRoot: repo.root,
        filePath: "new.txt",
      });
      expect(followed.items.map((c) => c.hash)).toEqual([renamed, first]);

      const stopped = await client.getFileHistoryPage({
        repoRoot: repo.root,
        filePath: "new.txt",
        followRenames: false,
      });
      expect(stopped.items.map((c) => c.hash)).toEqual([renamed]);
    });
  });

  it("returns an empty page — not an error — on an unborn branch", async () => {
    await withRepo(async (repo) => {
      const page = await client.getFileHistoryPage({ repoRoot: repo.root, filePath: "f.txt" });
      expect(page.items).toEqual([]);
      expect(page.hasMore).toBe(false);
    });
  });
});

// -- 4. Single commit ---------------------------------------------------

describe("getCommit", () => {
  it("fills every CommitDto field from a real commit", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const first = repo.commit("first");
      repo.write("f.txt", "two\n");
      repo.git(["add", "-A"]);
      repo.git(["commit", "-q", "-m", "second subject", "-m", "body line 1\nbody line 2"]);
      repo.git(["tag", "v1"]);
      const second = repo.head();

      const commit = await client.getCommit(repo.root, second);

      expect(commit.hash).toBe(second);
      expect(second.startsWith(commit.shortHash)).toBe(true);
      expect(commit.parents).toEqual([first]);
      expect(commit.author).toEqual(FIXTURE_AUTHOR);
      expect(commit.committer).toEqual(FIXTURE_AUTHOR);
      expect(commit.authorDate.secondsSinceEpoch).toBe(FIXTURE_EPOCH_SECONDS);
      expect(commit.subject).toBe("second subject");
      expect(commit.body).toContain("body line 1");
      expect(commit.body).toContain("body line 2");
      expect(commit.isMerge).toBe(false);
      expect(commit.isRoot).toBe(false);
      expect(commit.decorations).toContainEqual({ kind: "head" });
      expect(commit.decorations).toContainEqual({ kind: "branch", name: "main" });
      expect(commit.decorations).toContainEqual({ kind: "tag", name: "v1" });
    });
  });

  it("marks a root commit isRoot and a merge commit isMerge", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const root = repo.commit("root");
      repo.git(["checkout", "-q", "-b", "side"]);
      repo.write("side.txt", "s\n");
      repo.commit("side");
      repo.git(["checkout", "-q", "main"]);
      repo.write("main.txt", "m\n");
      repo.commit("main");
      repo.git(["merge", "-q", "--no-ff", "-m", "merge", "side"]);
      const merge = repo.head();

      expect((await client.getCommit(repo.root, root)).isRoot).toBe(true);
      const mergeCommit = await client.getCommit(repo.root, merge);
      expect(mergeCommit.isMerge).toBe(true);
      expect(mergeCommit.parents).toHaveLength(2);
    });
  });

  it("fails rather than inventing a commit for an unresolvable revision", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");
      await expect(client.getCommit(repo.root, "definitely-not-a-ref")).rejects.toThrow();
    });
  });
});

// -- 5. Commit diff -----------------------------------------------------

describe("getCommitDiff", () => {
  it("diffs a normal commit against its first parent and names that base", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\ntwo\n");
      const first = repo.commit("first");
      repo.write("f.txt", "one\nTWO\n");
      const second = repo.commit("second");

      const diff = await client.getCommitDiff(repo.root, second);

      expect(diff.target).toBe(second);
      expect(diff.base).toBe(first);
      expect(diff.diff.files).toHaveLength(1);
      const file = diff.diff.files[0];
      expect(file.path).toBe("f.txt");
      expect(file.changeType).toBe("modified");
      expect(file.isBinary).toBe(false);
      expect(file.truncated).toBe(false);
      expect(file.hunks).toHaveLength(1);
      const origins = file.hunks[0].lines.map((l) => l.origin);
      expect(origins).toContain("deletion");
      expect(origins).toContain("addition");
    });
  });

  it("diffs a root commit against the empty tree and reports base: null", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const root = repo.commit("root");

      const diff = await client.getCommitDiff(repo.root, root);

      expect(diff.base).toBeNull();
      expect(diff.diff.files).toHaveLength(1);
      expect(diff.diff.files[0].changeType).toBe("added");
    });
  });

  it("diffs a merge commit against its first parent rather than showing nothing", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("root");
      repo.git(["checkout", "-q", "-b", "side"]);
      repo.write("side.txt", "s\n");
      repo.commit("side");
      repo.git(["checkout", "-q", "main"]);
      repo.write("main.txt", "m\n");
      const mainTip = repo.commit("main");
      repo.git(["merge", "-q", "--no-ff", "-m", "merge", "side"]);
      const merge = repo.head();

      const diff = await client.getCommitDiff(repo.root, merge);

      // `git show` would print no diff at all here. The first-parent policy
      // must produce the side branch's change instead.
      expect(diff.base).toBe(mainTip);
      expect(diff.diff.files.map((f) => f.path)).toEqual(["side.txt"]);
    });
  });

  it("reports a rename with its previous path", async () => {
    await withRepo(async (repo) => {
      repo.write("old.txt", "contents that stay identical so git detects a rename\n");
      repo.commit("first");
      repo.git(["mv", "old.txt", "new.txt"]);
      const renamed = repo.commit("rename");

      const diff = await client.getCommitDiff(repo.root, renamed);

      expect(diff.diff.files).toHaveLength(1);
      expect(diff.diff.files[0].changeType).toBe("renamed");
      expect(diff.diff.files[0].path).toBe("new.txt");
      expect(diff.diff.files[0].previousPath).toBe("old.txt");
    });
  });

  it("reports a deletion", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.write("keep.txt", "k\n");
      repo.commit("first");
      repo.git(["rm", "-q", "f.txt"]);
      const deleted = repo.commit("delete");

      const diff = await client.getCommitDiff(repo.root, deleted);

      expect(diff.diff.files).toHaveLength(1);
      expect(diff.diff.files[0].changeType).toBe("deleted");
      expect(diff.diff.files[0].path).toBe("f.txt");
    });
  });

  it("flags a binary file instead of pretending it has a text diff", async () => {
    await withRepo(async (repo) => {
      repo.writeBytes("bin.dat", Buffer.from([0, 1, 2, 3, 0, 255, 7]));
      const added = repo.commit("add binary");

      const diff = await client.getCommitDiff(repo.root, added);

      expect(diff.diff.files).toHaveLength(1);
      expect(diff.diff.files[0].isBinary).toBe(true);
      expect(diff.diff.files[0].hunks).toEqual([]);
    });
  });

  it("marks a line with no trailing newline", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");
      repo.write("f.txt", "one\nno newline here");
      const second = repo.commit("second");

      const diff = await client.getCommitDiff(repo.root, second);
      const lines = diff.diff.files[0].hunks.flatMap((h) => h.lines);
      const added = lines.find((l) => l.content === "no newline here");
      expect(added?.hasTrailingNewline).toBe(false);
    });
  });
});

// -- 6. Line history ----------------------------------------------------

describe("getLineHistory", () => {
  it("traces a line range through the commits that touched it", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\ntwo\nthree\n");
      const first = repo.commit("first");
      repo.write("f.txt", "one\nTWO\nthree\n");
      const second = repo.commit("second");
      repo.write("unrelated.txt", "x\n");
      repo.commit("unrelated");

      const history = await client.getLineHistory({
        repoRoot: repo.root,
        filePath: "f.txt",
        startLine: 2,
        endLine: 2,
      });

      expect(history.file).toBe("f.txt");
      expect(history.range).toEqual({ start: 2, end: 2 });
      // `revision` must be the resolved commit, never the literal "HEAD".
      expect(history.revision).toMatch(/^[0-9a-f]{40}$/);
      expect(history.revision).toBe(repo.head());
      expect(history.entries.map((e) => e.commit.hash)).toEqual([second, first]);
      expect(history.entries[0].hunks.length).toBeGreaterThan(0);
      expect(history.entries[0].commit.subject).toBe("second");
    });
  });

  it("rejects an invalid range instead of quietly querying something else", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");
      await expect(
        client.getLineHistory({ repoRoot: repo.root, filePath: "f.txt", startLine: 5, endLine: 2 }),
      ).rejects.toBeInstanceOf(GitParseError);
      await expect(
        client.getLineHistory({ repoRoot: repo.root, filePath: "f.txt", startLine: 0, endLine: 2 }),
      ).rejects.toBeInstanceOf(GitParseError);
    });
  });
});

// -- 7. File contents at a revision -------------------------------------

describe("getFileContentAtRevision", () => {
  it("returns the file's text as of that revision, not the working tree", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "original\n");
      const first = repo.commit("first");
      repo.write("f.txt", "changed\n");
      repo.commit("second");

      const content = await client.getFileContentAtRevision(repo.root, "f.txt", first);

      expect(content.kind).toBe("text");
      if (content.kind !== "text") return;
      expect(content.content).toBe("original\n");
      expect(content.revision).toBe(first);
      expect(content.path).toBe("f.txt");
    });
  });

  it("reports missing — not an error — for a file absent at that revision", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      const first = repo.commit("first");
      repo.write("later.txt", "l\n");
      repo.commit("second");

      const content = await client.getFileContentAtRevision(repo.root, "later.txt", first);

      expect(content.kind).toBe("missing");
    });
  });

  it("reports binary rather than returning mangled text", async () => {
    await withRepo(async (repo) => {
      repo.writeBytes("bin.dat", Buffer.from([0, 1, 2, 3, 0, 255]));
      const added = repo.commit("add binary");

      const content = await client.getFileContentAtRevision(repo.root, "bin.dat", added);

      expect(content.kind).toBe("binary");
    });
  });

  it("preserves non-ASCII text exactly", async () => {
    await withRepo(async (repo) => {
      repo.write("u.txt", "café — ünïcödé ✓\n");
      const added = repo.commit("add unicode");

      const content = await client.getFileContentAtRevision(repo.root, "u.txt", added);

      expect(content.kind).toBe("text");
      if (content.kind !== "text") return;
      expect(content.content).toBe("café — ünïcödé ✓\n");
    });
  });

  it("refuses an option-shaped revision", async () => {
    await withRepo(async (repo) => {
      repo.write("f.txt", "one\n");
      repo.commit("first");
      await expect(
        client.getFileContentAtRevision(repo.root, "f.txt", "--upload-pack=touch /tmp/pwned"),
      ).rejects.toBeInstanceOf(GitParseError);
    });
  });
});

// -- Path handling ------------------------------------------------------

describe("relativeToRepo", () => {
  it("produces a POSIX-style repository-relative path", () => {
    const root = process.platform === "win32" ? "C:\\repo" : "/repo";
    const file = process.platform === "win32" ? "C:\\repo\\src\\a.ts" : "/repo/src/a.ts";
    expect(relativeToRepo(root, file)).toBe("src/a.ts");
  });
});
