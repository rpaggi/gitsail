// Associating the active file with "its" repository (US-068).
//
// Before ADR-025 this module's contract was "never inspect the filesystem
// for `.git` yourself — ask `gitsail open`". That rule survives ADR-025
// almost intact, and it is worth being precise about what changed: this
// module still never walks directories looking for `.git`, still never
// stats a path, and still never decides for itself what counts as a
// repository. It asks `git rev-parse` — i.e. Git itself — through
// `git/gitClient.ts`, and forwards whatever answer comes back. What it no
// longer does is route that question through a second GitSail process.

import path from "node:path";

import { GitClient } from "./git/gitClient";
import type { RepositoryDto } from "./dto";

export interface WorkspaceFolderLike {
  uri: { fsPath: string };
}

export interface DocumentUriLike {
  scheme: string;
  fsPath: string;
}

export type RepositoryContext =
  | { kind: "repository"; queryRoot: string; repository: RepositoryDto }
  /** A valid, structured "not a repository" answer — the expected, silent
   * case (US-068 criterion 3), never an error. */
  | { kind: "no-repository"; queryRoot: string }
  /** No active editor, or a document that is not a plain file on disk
   * (untitled, output channel, diff view, ...) — nothing to resolve. */
  | { kind: "no-context" }
  /** Git itself could not be run or could not be trusted (missing
   * executable, unreadable output, timeout) — an environment-level problem,
   * never shown per-file/repeatedly by this module. */
  | { kind: "git-unavailable"; queryRoot: string; error: Error };

interface CacheEntry {
  promise: Promise<RepositoryContext>;
}

/**
 * Resolves and caches "which repository does this query root belong to,
 * if any".
 *
 * Caches by query root (US-068 criterion 3): re-focusing files under a
 * folder that was already resolved — including a folder with no
 * repository — never re-runs `git` or re-surfaces a failure on every
 * keystroke/focus change. Call `invalidateAll()` whenever something that
 * could actually change the answer changes (workspace folders, workspace
 * trust) — the extension controller owns that decision, not this class.
 */
export class RepositoryContextResolver {
  private readonly cache = new Map<string, CacheEntry>();

  constructor(private readonly client: GitClient) {}

  invalidateAll(): void {
    this.cache.clear();
  }

  /**
   * Picks the directory to run discovery in for a given active document
   * (criterion 2: correct even across a multi-root workspace).
   *
   * - A document inside one of `workspaceFolders` resolves to that
   *   folder's root, regardless of how deeply nested the file is — `git
   *   rev-parse` walks upward from there itself.
   * - A document outside every workspace folder ("external file", e.g.
   *   opened directly via File > Open) still resolves — to its own
   *   containing directory — since it may belong to an entirely different
   *   repository on disk than anything in the workspace; this still never
   *   inspects `.git` itself, it only picks *where* to ask.
   * - `undefined` (no active editor, or a non-file document such as
   *   `untitled:`/`output:`) means there is nothing to resolve.
   */
  resolveQueryRoot(
    documentUri: DocumentUriLike | undefined,
    workspaceFolders: readonly WorkspaceFolderLike[],
  ): string | undefined {
    if (!documentUri || documentUri.scheme !== "file") {
      return undefined;
    }
    const folder = workspaceFolders.find((f) => isUnderPath(documentUri.fsPath, f.uri.fsPath));
    if (folder) {
      return folder.uri.fsPath;
    }
    return path.dirname(documentUri.fsPath);
  }

  resolve(queryRoot: string): Promise<RepositoryContext> {
    const cached = this.cache.get(queryRoot);
    if (cached) {
      return cached.promise;
    }
    const promise = this.query(queryRoot);
    this.cache.set(queryRoot, { promise });
    return promise;
  }

  private async query(queryRoot: string): Promise<RepositoryContext> {
    try {
      const outcome = await this.client.discover(queryRoot);
      // "This folder has no repository" is the one expected, silent outcome
      // (criterion 3) — `git rev-parse` exiting non-zero for a path that is
      // not inside a repository is a routine answer, not a fault. Anything
      // else that goes wrong (git missing, unparseable output, timeout) is
      // an environment problem worth surfacing, and takes the path below.
      if (outcome.kind === "no-repository") {
        return { kind: "no-repository", queryRoot };
      }
      return { kind: "repository", queryRoot, repository: outcome.repository };
    } catch (error) {
      return { kind: "git-unavailable", queryRoot, error: error as Error };
    }
  }
}

/** True when `candidate` is `root` itself or a descendant of it, comparing
 * with `path.relative` so it is correct for both POSIX and Windows-style
 * `fsPath` values without hand-rolling separator logic. */
function isUnderPath(candidate: string, root: string): boolean {
  const relative = path.relative(root, candidate);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}
