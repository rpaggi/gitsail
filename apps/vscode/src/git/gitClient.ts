// The seven read-only Git queries this extension needs, assembled as
// argument vectors and parsed into the DTOs in `../dto.ts`.
//
// This is the data-access layer ADR-025 created. It replaces
// `cliClient.ts` + `cliLocator.ts` + the per-query `gitsail <subcommand>`
// wrappers: instead of asking a `gitsail` binary, each method runs `git`
// through `process.ts` (the only module allowed to spawn it) and hands the
// output to one of the pure parsers beside this file. The DTOs it produces
// are byte-compatible with what `gitsail-cli --json` used to return, which
// is why nothing in the presentation layer (`blameFormat.ts`,
// `historyPresentation.ts`, `commitDetailsText.ts`, ...) changed.
//
// Argument safety, applied uniformly here rather than per call site:
//   - Paths are always preceded by `--`, so a filename can never be read as
//     a revision or an option.
//   - Revisions are always preceded by `--end-of-options`, so a
//     revision-shaped-but-actually-a-flag value (`--output=...`) fails as
//     an unresolvable revision instead of changing what the invocation
//     does. Where `git` does not accept `--end-of-options` — `git blame`
//     genuinely does not, verified against Git 2.43 — `assertPlainRevision`
//     rejects a leading `-` outright instead.
//   - Nothing is ever interpolated into a format string. The one place a
//     path and a revision must share an argument (`git show <rev>:<path>`
//     and `git log -L<a>,<b>:<path>`, both forced by Git's own syntax) is
//     built as a single argv element, never a shell word.

import path from "node:path";

import { GitParseError, GitProcessError } from "./errors";
import { parseBlamePorcelain } from "./parseBlame";
import { parseCommitRecord, parseCommitRecords, LOG_FORMAT, RECORD_SEP } from "./parseCommit";
import { parseDiff, parseDiffHunk, rawLines } from "./parseDiff";
import { decodeStdout, runGitChecked, tryRunGit } from "./process";
import {
  BlameDto,
  CommitDiffDto,
  CommitDto,
  DiffHunkDto,
  FileContentDto,
  HeadStateDto,
  LineHistoryDto,
  LineHistoryEntryDto,
  PageDto,
  RepositoryDto,
} from "../dto";

/** Git's well-known empty-tree object. Diffing a root commit against it is
 * how `git` itself expresses "everything in this commit is new" — the same
 * constant and the same policy as
 * `gitsail-application::use_cases::EMPTY_TREE_SHA1`. */
const EMPTY_TREE_SHA1 = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/** Default page size mirrors `gitsail-application`'s `DEFAULT_COMMIT_LIMIT`
 * so an unbounded history query behaves the same as it used to. */
const DEFAULT_COMMIT_LIMIT = 50;

/** Bytes sniffed for a NUL when classifying file content. Same value as
 * `gitsail-git`'s `BINARY_SNIFF_LEN`. */
const BINARY_SNIFF_LEN = 8000;

export interface QueryOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface BlameQueryArgs {
  repoRoot: string;
  /** Path relative to `repoRoot`, POSIX-separated. */
  filePath: string;
  /** `undefined` blames the working tree, including uncommitted changes. */
  revision?: string;
}

export interface CommitHistoryQueryArgs {
  repoRoot: string;
  /** Path relative to `repoRoot`. */
  filePath: string;
  revision?: string;
  /** Opaque cursor; today an offset, exactly as `gitsail log` returned. */
  cursor?: string;
  limit?: number;
  /** Defaults to following renames, matching the CLI's own default. */
  followRenames?: boolean;
}

export interface LineHistoryQueryArgs {
  repoRoot: string;
  filePath: string;
  startLine: number;
  endLine: number;
  revision?: string;
}

export type DiscoveryOutcome =
  | { kind: "repository"; repository: RepositoryDto }
  /** `queryRoot` is not inside any Git repository. A routine, silent answer
   * — never an error (it is what `repository_not_found` used to mean). */
  | { kind: "no-repository" };

export type GitAvailability =
  | { status: "ok"; version: string }
  | { status: "not-found" }
  /** Something answered `--version` but not recognizably as Git. */
  | { status: "unrecognized" };

/**
 * Runs `git --version` once to establish that Git is usable at all.
 *
 * Replaces `cliLocator.ts`'s `probeCliBinary`: there is no configured path
 * to resolve and no minimum version to enforce here (the Core deliberately
 * enforces none either — SAD §39 leaves a minimum Git version open, and
 * ADR-021 records 2.31 as the project minimum rather than a runtime gate),
 * so this only answers "can we run git, and is it actually git".
 */
export async function probeGit(cwd: string, options: QueryOptions = {}): Promise<GitAvailability> {
  let result;
  try {
    result = await runGitChecked({ cwd, args: ["--version"], ...options });
  } catch (error) {
    if ((error as Error).name === "GitNotFoundError") {
      return { status: "not-found" };
    }
    return { status: "unrecognized" };
  }
  const version = parseGitVersion(decodeStdout(result));
  return version === undefined ? { status: "unrecognized" } : { status: "ok", version };
}

/** Parses `git version X.Y.Z[...]` into just the version token, mirroring
 * `gitsail-git`'s `parse_git_version`. */
export function parseGitVersion(output: string): string | undefined {
  const rest = output.trim();
  if (!rest.startsWith("git version ")) {
    return undefined;
  }
  const version = rest.slice("git version ".length).split(/\s+/)[0];
  return version && version.length > 0 ? version : undefined;
}

/**
 * Every read-only Git query the extension makes.
 *
 * One instance is created per resolved repository binding by
 * `controller.ts` and shared with `historyController.ts`, exactly as the
 * single `GitSailCliClient` instance used to be.
 */
export class GitClient {
  constructor(private readonly defaults: QueryOptions = {}) {}

  private merge(options: QueryOptions | undefined): QueryOptions {
    return { ...this.defaults, ...options };
  }

  // -- 1. Repository discovery --------------------------------------------

  /**
   * `git rev-parse` discovery, replacing `gitsail open`.
   *
   * Two invocations, not one, for the same reason the Rust adapter needs
   * two: `--show-toplevel` fails outright in a bare repository (there is no
   * working tree), while `--is-bare-repository`/`--absolute-git-dir`
   * succeed for both. Asking for all three at once would therefore make
   * every bare repository look like "not a repository".
   */
  async discover(queryRoot: string, options?: QueryOptions): Promise<DiscoveryOutcome> {
    const identity = await tryRunGit({
      cwd: queryRoot,
      args: ["rev-parse", "--path-format=absolute", "--is-bare-repository", "--absolute-git-dir"],
      ...this.merge(options),
    });
    if (identity === undefined) {
      // Non-zero here is `git`'s way of saying "not a repository", which is
      // a legitimate answer for any folder the user happens to open.
      return { kind: "no-repository" };
    }

    const identityLines = decodeStdout(identity)
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l.length > 0);
    const bareText = identityLines[0];
    if (bareText !== "true" && bareText !== "false") {
      throw new GitParseError("Could not parse git rev-parse --is-bare-repository output.");
    }
    const isBare = bareText === "true";
    const gitDir = identityLines[1];
    if (gitDir === undefined) {
      throw new GitParseError("git rev-parse did not report an absolute git dir.");
    }

    let rootPath: string;
    if (isBare) {
      rootPath = gitDir;
    } else {
      const toplevel = await runGitChecked({
        cwd: queryRoot,
        args: ["rev-parse", "--path-format=absolute", "--show-toplevel"],
        ...this.merge(options),
      });
      rootPath = decodeStdout(toplevel).trim();
    }

    const headState = await this.determineHeadState(queryRoot, options);
    return {
      kind: "repository",
      repository: {
        // `RepositoryId::from_canonical_root` is literally the root path as
        // a string — reproduced here rather than invented, so a repository
        // identifies itself identically in the extension and the Core.
        id: rootPath,
        rootPath,
        worktreePath: isBare ? null : rootPath,
        isBare,
        headState,
        currentBranch: headState.state === "attached" ? headState.branch : null,
      },
    };
  }

  private async determineHeadState(cwd: string, options?: QueryOptions): Promise<HeadStateDto> {
    const hasHead = await tryRunGit({
      cwd,
      args: ["rev-parse", "--verify", "-q", "HEAD"],
      ...this.merge(options),
    });
    if (hasHead === undefined) {
      return { state: "unborn" };
    }
    const symbolic = await tryRunGit({
      cwd,
      args: ["symbolic-ref", "-q", "--short", "HEAD"],
      ...this.merge(options),
    });
    if (symbolic !== undefined) {
      return { state: "attached", branch: decodeStdout(symbolic).trim() };
    }
    return { state: "detached", commit: decodeStdout(hasHead).trim() };
  }

  // -- 2. Blame ------------------------------------------------------------

  /**
   * `git blame --porcelain`, replacing `gitsail blame`.
   *
   * `revision` echoes straight back into the returned DTO rather than being
   * resolved: `BlameDto.revision` has always meant "what was asked for",
   * and `null` (the working tree, including uncommitted lines) is the only
   * value the extension actually uses today.
   */
  async getBlame(query: BlameQueryArgs, options?: QueryOptions): Promise<BlameDto> {
    const args = ["blame", "--porcelain"];
    if (query.revision !== undefined) {
      // `git blame` rejects `--end-of-options` (verified against Git
      // 2.43), so a flag-shaped revision is refused outright instead.
      assertPlainRevision(query.revision);
      args.push(query.revision);
    }
    args.push("--", query.filePath);

    const result = await runGitChecked({ cwd: query.repoRoot, args, ...this.merge(options) });
    if (result.truncated) {
      throw new GitParseError(
        "The blame output for this file was too large to read completely; GitSail will not show partial authorship.",
      );
    }
    return {
      file: query.filePath,
      revision: query.revision ?? null,
      lines: parseBlamePorcelain(decodeStdout(result)),
    };
  }

  // -- 3. Commit history / file history ------------------------------------

  /**
   * `git log`, replacing `gitsail log --path`.
   *
   * Pagination reproduces the CLI's own offset cursor exactly (request
   * `limit + 1` records, and if the extra one arrives report `hasMore` and
   * a `nextCursor` of `offset + limit`), so a caller that stored a cursor
   * still means the same thing by it.
   */
  async getFileHistoryPage(
    query: CommitHistoryQueryArgs,
    options?: QueryOptions,
  ): Promise<PageDto<CommitDto>> {
    const limit = query.limit ?? DEFAULT_COMMIT_LIMIT;
    if (!Number.isInteger(limit) || limit <= 0) {
      throw new GitParseError("A history page limit must be a positive integer.");
    }
    const offset = parseCursor(query.cursor);

    const args = [
      "log",
      `--pretty=format:${LOG_FORMAT}`,
      "--date=raw",
      "--no-color",
      "-n",
      String(limit + 1),
      `--skip=${offset}`,
    ];
    // `--follow` only makes sense (and is only accepted) with a single
    // pathspec, which this query always has. It must precede
    // `--end-of-options`.
    if (query.followRenames !== false) {
      args.push("--follow");
    }
    args.push("--end-of-options", query.revision ?? "HEAD", "--", query.filePath);

    const result = await tryRunGit({ cwd: query.repoRoot, args, ...this.merge(options) });
    if (result === undefined) {
      // `git log` that cannot resolve its revision (e.g. `HEAD` on an
      // unborn branch) is an empty page, not a failure — a legitimate
      // repository state.
      return { items: [], hasMore: false };
    }

    const items = parseCommitRecords(decodeStdout(result));
    const hasMore = items.length > limit;
    if (hasMore) {
      items.length = limit;
    }
    return hasMore
      ? { items, hasMore: true, nextCursor: String(offset + limit) }
      : { items, hasMore: false };
  }

  // -- 4. Single commit detail ---------------------------------------------

  /** `git show -s`, replacing `gitsail commit`. */
  async getCommit(
    repoRoot: string,
    revision: string,
    options?: QueryOptions,
  ): Promise<CommitDto> {
    const result = await tryRunGit({
      cwd: repoRoot,
      args: [
        "show",
        "-s",
        `--format=${LOG_FORMAT}`,
        "--date=raw",
        "--no-color",
        "--end-of-options",
        revision,
      ],
      ...this.merge(options),
    });
    if (result === undefined) {
      throw new GitProcessError(`No commit found for ${revision}.`, null, "");
    }
    const record = decodeStdout(result);
    const first = record.split(RECORD_SEP)[0];
    if (first === undefined || first.length === 0) {
      throw new GitParseError(`No commit found for ${revision}.`);
    }
    return parseCommitRecord(first);
  }

  // -- 5. Commit diff ------------------------------------------------------

  /**
   * `git diff <base> <target>`, replacing `gitsail commit-diff`.
   *
   * Base policy is reproduced from `GetCommitDiff`, not re-invented: a root
   * commit is compared against the empty tree and reports `base: null`; any
   * other commit — including a merge — is compared against its **first**
   * parent, which is then named in `base` so the UI can say which
   * comparison it actually showed.
   *
   * `git diff` rather than `git show --patch` on purpose: `git show` prints
   * *no* diff at all for a merge commit by default (it suppresses the
   * combined diff), which would silently turn every merge into "no file
   * changes". Diffing explicitly against the resolved first parent is the
   * only form that implements the policy above for merges as well.
   */
  async getCommitDiff(
    repoRoot: string,
    revision: string,
    options?: QueryOptions,
  ): Promise<CommitDiffDto> {
    const commit = await this.getCommit(repoRoot, revision, options);
    const base = commit.isRoot ? null : commit.parents[0];
    const from = base ?? EMPTY_TREE_SHA1;

    const result = await runGitChecked({
      cwd: repoRoot,
      args: [
        // A global option, so it must precede the subcommand. Without it
        // Git C-style-quotes any path byte >= 0x80 in the header lines this
        // parser reads, and the parser does not unquote them.
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-color",
        // Never run a user-configured external diff driver: repository
        // configuration is untrusted input.
        "--no-ext-diff",
        "-M",
        "-U3",
        "--end-of-options",
        from,
        commit.hash,
      ],
      ...this.merge(options),
    });

    return { target: commit.hash, base, diff: parseDiff(decodeStdout(result)) };
  }

  // -- 6. Line history -----------------------------------------------------

  /**
   * `git log -L <start>,<end>:<file>`, replacing `gitsail line-history`.
   *
   * `revision` is resolved to a real commit up front (rather than echoing
   * the literal string "HEAD") because `LineHistoryDto.revision` is
   * non-nullable and has always carried the commit that was actually
   * traced.
   *
   * Known limitation inherited from `git log -L`'s own syntax, not
   * introduced here: a path containing a colon cannot be expressed this
   * way.
   */
  async getLineHistory(
    query: LineHistoryQueryArgs,
    options?: QueryOptions,
  ): Promise<LineHistoryDto> {
    const { startLine, endLine } = query;
    if (!Number.isInteger(startLine) || !Number.isInteger(endLine) || startLine < 1 || startLine > endLine) {
      throw new GitParseError(
        `Invalid line range ${startLine}-${endLine}: start must be at least 1 and not greater than end.`,
      );
    }

    const revision = await this.resolveRevision(query.repoRoot, query.revision ?? "HEAD", options);
    const result = await runGitChecked({
      cwd: query.repoRoot,
      args: [
        "log",
        `-L${startLine},${endLine}:${query.filePath}`,
        "--no-color",
        `--pretty=format:%H${RECORD_SEP}`,
        "--end-of-options",
        revision,
      ],
      ...this.merge(options),
    });

    const entries: LineHistoryEntryDto[] = [];
    for (const block of splitLineHistoryBlocks(decodeStdout(result))) {
      entries.push({
        commit: await this.getCommit(query.repoRoot, block.hash, options),
        hunks: parseLineHistoryHunks(block.lines),
      });
    }

    return {
      file: query.filePath,
      revision,
      range: { start: startLine, end: endLine },
      entries,
    };
  }

  /** `git rev-parse --verify -q --end-of-options <rev>^{commit}`. `^{commit}`
   * peels a tag down to a commit and makes anything that is not a commit
   * fail, so a caller always gets an unambiguous commit or a clear error. */
  async resolveRevision(
    repoRoot: string,
    revision: string,
    options?: QueryOptions,
  ): Promise<string> {
    const result = await tryRunGit({
      cwd: repoRoot,
      args: ["rev-parse", "--verify", "-q", "--end-of-options", `${revision}^{commit}`],
      ...this.merge(options),
    });
    if (result === undefined) {
      throw new GitProcessError(
        `Revision '${revision}' could not be resolved to a commit.`,
        null,
        "",
      );
    }
    return decodeStdout(result).trim();
  }

  // -- 7. File contents at a revision --------------------------------------

  /**
   * `git show <rev>:<path>`, replacing `gitsail show-file`.
   *
   * `binary` and `missing` are legitimate outcomes, never errors — which is
   * exactly what `FileContentDto`'s tagged union has always encoded, and
   * why `historyController.ts` renders all three kinds explicitly.
   */
  async getFileContentAtRevision(
    repoRoot: string,
    filePath: string,
    revision: string,
    options?: QueryOptions,
  ): Promise<FileContentDto> {
    assertPlainRevision(revision);
    const result = await tryRunGit({
      cwd: repoRoot,
      // `<rev>:<path>` is one argv element because Git's own syntax
      // requires it; it is still never a shell word.
      args: ["show", "--end-of-options", `${revision}:${filePath}`],
      ...this.merge(options),
    });
    if (result === undefined) {
      return { kind: "missing", path: filePath, revision };
    }
    if (isBinaryContent(result.stdout)) {
      return { kind: "binary", path: filePath, revision };
    }
    return {
      kind: "text",
      path: filePath,
      revision,
      content: result.stdout.toString("utf8"),
    };
  }
}

// -- helpers ---------------------------------------------------------------

/** Rejects a revision that could be read as an option, for the call sites
 * where `git` does not accept `--end-of-options`. */
function assertPlainRevision(revision: string): void {
  if (revision.startsWith("-")) {
    throw new GitParseError(`Refusing to use '${revision}' as a revision: it looks like an option.`);
  }
}

function parseCursor(cursor: string | undefined): number {
  if (cursor === undefined) {
    return 0;
  }
  const offset = Number(cursor);
  if (!Number.isInteger(offset) || offset < 0) {
    throw new GitParseError("History cursor was not a valid offset.");
  }
  return offset;
}

/** Same classification as `gitsail-git`'s `classify_file_content`: a NUL in
 * the first `BINARY_SNIFF_LEN` bytes, or content that is not valid UTF-8 at
 * all, is binary. */
function isBinaryContent(bytes: Buffer): boolean {
  const sniffLength = Math.min(bytes.length, BINARY_SNIFF_LEN);
  for (let i = 0; i < sniffLength; i += 1) {
    if (bytes[i] === 0) {
      return true;
    }
  }
  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return false;
  } catch {
    return true;
  }
}

/** A `git log -L --pretty=format:%H<RS>` header line is exactly a hash
 * immediately followed by the record separator and nothing else —
 * unambiguous, since a diff line can never take that shape. */
function parseLineHistoryHeader(line: string): string | undefined {
  if (!line.endsWith(RECORD_SEP)) {
    return undefined;
  }
  const hash = line.slice(0, -RECORD_SEP.length);
  return /^[0-9a-f]{40,64}$/.test(hash) ? hash : undefined;
}

function splitLineHistoryBlocks(raw: string): { hash: string; lines: string[] }[] {
  const blocks: { hash: string; lines: string[] }[] = [];
  for (const line of rawLines(raw)) {
    const hash = parseLineHistoryHeader(line);
    if (hash !== undefined) {
      blocks.push({ hash, lines: [] });
    } else if (blocks.length > 0) {
      blocks[blocks.length - 1].lines.push(line);
    }
    // A line before the first header is discarded rather than misread as
    // diff content belonging to no commit.
  }
  return blocks;
}

/** Parses every `@@ ... @@` hunk in a commit's `-L` block, reusing the same
 * hunk grammar as an ordinary diff (the two are textually identical). */
function parseLineHistoryHunks(lines: readonly string[]): DiffHunkDto[] {
  const hunks: DiffHunkDto[] = [];
  let i = 0;
  while (i < lines.length) {
    if (lines[i].startsWith("@@ ")) {
      const { hunk, consumed } = parseDiffHunk(lines, i);
      hunks.push(hunk);
      i += consumed;
    } else {
      i += 1;
    }
  }
  return hunks;
}

/** `path.relative`, then normalized to forward slashes — every path
 * argument this client takes is a POSIX-style repository-relative path,
 * and `path.relative` is what makes that correct on Windows (drive
 * letters, backslashes) instead of string-prefix slicing. */
export function relativeToRepo(repoRoot: string, absolutePath: string): string {
  return path.relative(repoRoot, absolutePath).split(path.sep).join("/");
}
