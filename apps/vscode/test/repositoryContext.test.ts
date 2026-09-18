import { describe, expect, it, vi } from "vitest";

import { CliNotFoundError } from "../src/cliErrors";
import { Envelope } from "../src/protocol";
import { RepositoryContextResolver } from "../src/repositoryContext";

/** A `GitSailCliClient`-shaped stub: these tests are about
 * `RepositoryContextResolver`'s own resolution/caching logic, never about
 * process spawning (that is `cliClient.test.ts`'s job) — so `run` is a
 * plain function double instead of a real client. */
function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

const okEnvelope = (rootPath: string): Envelope<unknown> => ({
  status: "ok",
  schemaVersion: 1,
  requestId: "req-1",
  data: {
    id: rootPath,
    rootPath,
    worktreePath: rootPath,
    isBare: false,
    headState: { state: "attached", branch: "main" },
    currentBranch: "main",
  },
});

const notFoundEnvelope: Envelope<unknown> = {
  status: "error",
  schemaVersion: 1,
  requestId: "req-1",
  error: { code: "repository_not_found", message: "not a Git repository" },
};

describe("RepositoryContextResolver.resolveQueryRoot (US-068 criterion 2)", () => {
  it("resolves to nothing when there is no active document", () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
    expect(resolver.resolveQueryRoot(undefined, [])).toBeUndefined();
  });

  it("resolves to nothing for a non-file document (e.g. untitled/output)", () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
    expect(
      resolver.resolveQueryRoot({ scheme: "untitled", fsPath: "/whatever" }, []),
    ).toBeUndefined();
  });

  it("resolves nested files to their containing workspace folder, not the file itself", () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
    const folders = [{ uri: { fsPath: "/workspace/project-a" } }];
    const root = resolver.resolveQueryRoot(
      { scheme: "file", fsPath: "/workspace/project-a/src/deep/file.ts" },
      folders,
    );
    expect(root).toBe("/workspace/project-a");
  });

  it("picks the correct folder in a multi-root workspace", () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
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
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
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
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/x")));
    const folders = [{ uri: { fsPath: "/workspace/project-a" } }];
    const root = resolver.resolveQueryRoot(
      { scheme: "file", fsPath: "/elsewhere/other-repo/file.ts" },
      folders,
    );
    expect(root).toBe("/elsewhere/other-repo");
  });
});

describe("RepositoryContextResolver.resolve (US-068 criteria 1 and 3)", () => {
  it("returns a repository context on a valid ok envelope", async () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => okEnvelope("/repo")));
    const context = await resolver.resolve("/repo");
    expect(context).toEqual({
      kind: "repository",
      queryRoot: "/repo",
      repository: (okEnvelope("/repo") as Envelope<unknown> & { status: "ok" }).data,
    });
  });

  it("returns no-repository (never an error) for repository_not_found", async () => {
    const resolver = new RepositoryContextResolver(stubClient(async () => notFoundEnvelope));
    const context = await resolver.resolve("/not-a-repo");
    expect(context).toEqual({ kind: "no-repository", queryRoot: "/not-a-repo" });
  });

  it("treats any other domain error as a CLI-level problem, not a routine no-repository", async () => {
    const resolver = new RepositoryContextResolver(
      stubClient(async () => ({
        status: "error",
        schemaVersion: 1,
        requestId: "req-1",
        error: { code: "git_not_installed", message: "git executable not found" },
      })),
    );
    const context = await resolver.resolve("/repo");
    expect(context.kind).toBe("cli-unavailable");
  });

  it("surfaces a thrown client-level error as cli-unavailable", async () => {
    const resolver = new RepositoryContextResolver(
      stubClient(async () => {
        throw new CliNotFoundError("/opt/gitsail/gitsail");
      }),
    );
    const context = await resolver.resolve("/repo");
    expect(context.kind).toBe("cli-unavailable");
    if (context.kind === "cli-unavailable") {
      expect(context.error).toBeInstanceOf(CliNotFoundError);
    }
  });

  it("caches by query root: a second resolve() for the same root never re-invokes the client", async () => {
    const run = vi.fn(async () => okEnvelope("/repo"));
    const resolver = new RepositoryContextResolver(stubClient(run));

    await resolver.resolve("/repo");
    await resolver.resolve("/repo");

    expect(run).toHaveBeenCalledTimes(1);
  });

  it("queries independently per root, and invalidateAll() forces a fresh query", async () => {
    const run = vi.fn(async () => okEnvelope("/repo"));
    const resolver = new RepositoryContextResolver(stubClient(run));

    await resolver.resolve("/repo-a");
    await resolver.resolve("/repo-b");
    expect(run).toHaveBeenCalledTimes(2);

    resolver.invalidateAll();
    await resolver.resolve("/repo-a");
    expect(run).toHaveBeenCalledTimes(3);
  });
});
