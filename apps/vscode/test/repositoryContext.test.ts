// `RepositoryContextResolver`'s own resolution and caching logic.
//
// `resolveQueryRoot` is pure path arithmetic and its tests are unchanged
// from before ADR-025 — it decides *where* to ask, and that decision never
// involved the CLI.
//
// `resolve` did change: it no longer reads a `repository_not_found` code
// out of a JSON envelope, it interprets `git rev-parse`'s exit status. The
// three-way outcome it must produce is the same one it always had, so each
// of those tests survives with the same name and the same meaning:
//  - a real repository resolves to `repository`;
//  - "no repository here" is a routine, silent answer, never an error;
//  - anything else (git missing, unreadable output) is `git-unavailable`,
//    which is what `cli-unavailable` was called when the thing that could
//    be unavailable was the CLI.

import { describe, expect, it, vi } from "vitest";

import { GitClient } from "../src/git/gitClient";
import { GitNotFoundError } from "../src/git/errors";
import { RepositoryContextResolver } from "../src/repositoryContext";
import { TempRepo } from "./support/tempRepo";

/** A `GitClient`-shaped stub, for the tests that are about the resolver's
 * caching rather than about Git itself (those use a real repository). */
function stubClient(discover: GitClient["discover"]): GitClient {
  return { discover: vi.fn(discover) } as unknown as GitClient;
}

const anyRepository = async () =>
  ({
    kind: "repository" as const,
    repository: {
      id: "/repo",
      rootPath: "/repo",
      worktreePath: "/repo",
      isBare: false,
      headState: { state: "attached" as const, branch: "main" },
      currentBranch: "main",
    },
  });

describe("RepositoryContextResolver.resolveQueryRoot (US-068 criterion 2)", () => {
  it("resolves to nothing when there is no active document", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    expect(resolver.resolveQueryRoot(undefined, [])).toBeUndefined();
  });

  it("resolves to nothing for a non-file document (e.g. untitled/output)", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    expect(
      resolver.resolveQueryRoot({ scheme: "untitled", fsPath: "/whatever" }, []),
    ).toBeUndefined();
  });

  it("resolves nested files to their containing workspace folder, not the file itself", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    const folders = [{ uri: { fsPath: "/workspace/project-a" } }];
    const root = resolver.resolveQueryRoot(
      { scheme: "file", fsPath: "/workspace/project-a/src/deep/file.ts" },
      folders,
    );
    expect(root).toBe("/workspace/project-a");
  });

  it("picks the correct folder in a multi-root workspace", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    const folders = [
      { uri: { fsPath: "/workspace/project-a" } },
      { uri: { fsPath: "/workspace/project-b" } },
    ];
    expect(
      resolver.resolveQueryRoot({ scheme: "file", fsPath: "/workspace/project-b/readme.md" }, folders),
    ).toBe("/workspace/project-b");
    expect(
      resolver.resolveQueryRoot({ scheme: "file", fsPath: "/workspace/project-a/readme.md" }, folders),
    ).toBe("/workspace/project-a");
  });

  it("never confuses sibling folders with a shared name prefix", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    const folders = [{ uri: { fsPath: "/workspace/project" } }];
    // "/workspace/project-2/file.ts" is NOT under "/workspace/project" even
    // though the string has it as a prefix — a naive `startsWith` check
    // without a separator would get this wrong.
    const root = resolver.resolveQueryRoot(
      { scheme: "file", fsPath: "/workspace/project-2/file.ts" },
      folders,
    );
    expect(root).toBe("/workspace/project-2");
  });

  it("resolves a file outside every workspace folder to its own directory", () => {
    const resolver = new RepositoryContextResolver(stubClient(anyRepository));
    const folders = [{ uri: { fsPath: "/workspace/project-a" } }];
    const root = resolver.resolveQueryRoot(
      { scheme: "file", fsPath: "/elsewhere/other-repo/file.ts" },
      folders,
    );
    expect(root).toBe("/elsewhere/other-repo");
  });
});

describe("RepositoryContextResolver.resolve (US-068 criteria 1 and 3)", () => {
  it("returns a repository context for a real repository", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "one\n");
      repo.commit("first");
      const resolver = new RepositoryContextResolver(new GitClient());

      const context = await resolver.resolve(repo.root);

      expect(context.kind).toBe("repository");
      if (context.kind !== "repository") return;
      expect(context.queryRoot).toBe(repo.root);
      expect(context.repository.currentBranch).toBe("main");
      expect(context.repository.isBare).toBe(false);
    } finally {
      repo.dispose();
    }
  });

  it("returns no-repository (never an error) for a directory that is not in a repository", async () => {
    const resolver = new RepositoryContextResolver(
      stubClient(async () => ({ kind: "no-repository" as const })),
    );
    const context = await resolver.resolve("/not-a-repo");
    expect(context).toEqual({ kind: "no-repository", queryRoot: "/not-a-repo" });
  });

  it("surfaces an environment-level failure as git-unavailable, not as a routine no-repository", async () => {
    const resolver = new RepositoryContextResolver(
      stubClient(async () => {
        throw new GitNotFoundError();
      }),
    );
    const context = await resolver.resolve("/repo");
    expect(context.kind).toBe("git-unavailable");
    if (context.kind === "git-unavailable") {
      expect(context.error).toBeInstanceOf(GitNotFoundError);
    }
  });

  it("caches by query root: a second resolve() for the same root never re-queries", async () => {
    const discover = vi.fn(anyRepository);
    const resolver = new RepositoryContextResolver(stubClient(discover));

    await resolver.resolve("/repo");
    await resolver.resolve("/repo");

    expect(discover).toHaveBeenCalledTimes(1);
  });

  it("caches a no-repository answer too, so an unrelated folder is not re-queried on every focus change", async () => {
    const discover = vi.fn(async () => ({ kind: "no-repository" as const }));
    const resolver = new RepositoryContextResolver(stubClient(discover));

    await resolver.resolve("/not-a-repo");
    await resolver.resolve("/not-a-repo");

    expect(discover).toHaveBeenCalledTimes(1);
  });

  it("queries independently per root, and invalidateAll() forces a fresh query", async () => {
    const discover = vi.fn(anyRepository);
    const resolver = new RepositoryContextResolver(stubClient(discover));

    await resolver.resolve("/repo-a");
    await resolver.resolve("/repo-b");
    expect(discover).toHaveBeenCalledTimes(2);

    resolver.invalidateAll();
    await resolver.resolve("/repo-a");
    expect(discover).toHaveBeenCalledTimes(3);
  });
});
