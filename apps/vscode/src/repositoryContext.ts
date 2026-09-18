// Associating the active file with "its" repository (US-068).
//
// This module never inspects the filesystem for `.git` itself (criterion
// 1) — it only ever asks `gitsail open --repo <dir> --json`, i.e. the same
// discovery `gitsail-cli` already exposes to every other client, and
// forwards whatever answer the Core gives back.

import path from "node:path";

import { GitSailCliClient } from "./cliClient";
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
  /** A valid, structured "not a repository" answer from the Core — the
   * expected, silent case (US-068 criterion 3), never an error. */
  | { kind: "no-repository"; queryRoot: string }
  /** No active editor, or a document that is not a plain file on disk
   * (untitled, output channel, diff view, ...) — nothing to resolve. */
  | { kind: "no-context" }
  /** The CLI itself could not be trusted (missing/incompatible binary, bad
   * schema, ...) — a client-level problem, never shown per-file/repeatedly
   * by this module; see `cliClient`'s error types. */
  | { kind: "cli-unavailable"; queryRoot: string; error: Error };

interface CacheEntry {
  promise: Promise<RepositoryContext>;
}

/**
 * Resolves and caches "which repository does this query root belong to,
 * if any" by delegating to `gitsail open` (criterion 1).
 *
 * Caches by query root (US-068 criterion 3): re-focusing files under a
 * folder that was already resolved — including a folder with no
 * repository — never re-invokes the CLI or re-surfaces a failure on every
 * keystroke/focus change. Call `invalidateAll()` whenever something that
 * could actually change the answer changes (workspace folders, binary
 * configuration, workspace trust) — the extension controller owns that
 * decision, not this class.
 */
export class RepositoryContextResolver {
  private readonly cache = new Map<string, CacheEntry>();

  constructor(private readonly client: GitSailCliClient) {}

  invalidateAll(): void {
    this.cache.clear();
  }

  /**
   * Picks the directory to ask `gitsail open` about for a given active
   * document (criterion 2: correct even across a multi-root workspace).
   *
   * - A document inside one of `workspaceFolders` resolves to that
   *   folder's root, regardless of how deeply nested the file is —
   *   `gitsail open` walks upward from there itself.
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
      const envelope = await this.client.run<RepositoryDto>(["open", "--repo", queryRoot]);
      if (envelope.status === "ok") {
        return { kind: "repository", queryRoot, repository: envelope.data };
      }
      // `repository_not_found` (`gitsail_domain::ErrorCode`) is the one
      // expected, silent "this folder has no repository" outcome
      // (criterion 3). Any other domain error `open` could report (e.g.
      // `git_not_installed`, `internal`) is an environment/CLI-level
      // problem worth surfacing, not a routine "no repo here" — it takes
      // the same path as a client-level failure below.
      if (envelope.error.code === "repository_not_found") {
        return { kind: "no-repository", queryRoot };
      }
      return { kind: "cli-unavailable", queryRoot, error: new Error(envelope.error.message) };
    } catch (error) {
      return { kind: "cli-unavailable", queryRoot, error: error as Error };
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
