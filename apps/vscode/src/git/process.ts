// THE ONE MODULE IN THIS EXTENSION ALLOWED TO SPAWN A PROCESS NAMED `git`.
//
// `scripts/ci/check-architecture.sh` allowlists exactly this file by path
// (see the commented exception there); every other TypeScript file under
// `apps/` that spawns `git` is a build failure. Everything else in
// `src/git/` is a pure parser that is handed a string.
//
// ADR-025 moved the VS Code extension off the `gitsail` CLI binary and onto
// `git` directly. That makes this file the extension's process boundary,
// and it deliberately reproduces every safety property
// `crates/gitsail-git/src/runner.rs` already established for the Rust core
// — the bar was set there, this matches it rather than inventing a laxer
// one:
//
//   1. Never through a shell. `spawn(..., { shell: false })` with an
//      argument array, so nothing in a path, revision or filename can be
//      reinterpreted as shell syntax. Argument assembly (`gitClient.ts`)
//      additionally always separates paths with `--` and revisions with
//      `--end-of-options`.
//   2. A timeout, and cancellation via `AbortSignal`, both converging on
//      one `settle` path that tears down listeners/timers exactly once and
//      kills the child — so an abandoned blame query (the cursor moved
//      again) is actually killed, not left running.
//   3. Captured output is capped (`MAX_CAPTURED_STREAM_BYTES`) so a
//      pathological repository cannot exhaust the extension host's memory.
//      Bytes past the cap are still drained, never left to block the pipe.
//   4. stderr is redacted (`redact.ts`) before it is stored anywhere, let
//      alone shown: `git`'s own stderr can contain a credential-bearing URL.
//   5. stdin is `ignore` and `GIT_TERMINAL_PROMPT=0`, so a repository that
//      would otherwise prompt for credentials fails fast instead of
//      hanging an editor query forever.

import { spawn } from "node:child_process";

import { GitCancelledError, GitNotFoundError, GitProcessError, GitTimeoutError } from "./errors";
import { redactSecrets } from "./redact";

/** Per-stream cap on captured stdout/stderr. Mirrors
 * `gitsail-git`'s own `MAX_CAPTURED_STREAM_BYTES` exactly (8 MiB). */
export const MAX_CAPTURED_STREAM_BYTES = 8 * 1024 * 1024;

/** Default wall-clock budget for a single read-only query. Every call site
 * may override it; none may opt out of having one. */
export const DEFAULT_GIT_TIMEOUT_MS = 20_000;

export interface GitRunRequest {
  /** Working directory the query runs in. Always an absolute path chosen by
   * this extension (a repository root or a document's own directory) —
   * never assembled from repository content. */
  cwd: string;
  /** Argument vector, passed to `spawn` as-is. Never joined into a string. */
  args: readonly string[];
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface GitRunResult {
  /** Raw bytes, not decoded: `git show <rev>:<path>` may legitimately
   * return binary content that must be classified by byte, not by a lossy
   * UTF-8 round trip. Callers that want text call `decodeStdout`. */
  stdout: Buffer;
  /** Already redacted (`redact.ts`) and decoded. */
  stderr: string;
  exitCode: number | null;
  /** True when stdout exceeded `MAX_CAPTURED_STREAM_BYTES` and was cut off.
   * Surfaced rather than swallowed so a parser can refuse to present a
   * half-read result as a complete one. */
  truncated: boolean;
}

export function decodeStdout(result: GitRunResult): string {
  return result.stdout.toString("utf8");
}

/** Environment overrides applied to every invocation, on top of the
 * inherited environment (which must be kept: PATH, HOME and the user's
 * credential/config setup all live there).
 *
 * - `GIT_TERMINAL_PROMPT=0` / `GIT_ASKPASS` / `SSH_ASKPASS`: a read-only
 *   editor query must never block on an interactive credential prompt the
 *   user cannot even see. It fails instead, and the timeout is a backstop.
 * - `GIT_OPTIONAL_LOCKS=0`: these are all read-only queries fired on cursor
 *   movement; they must not take the index lock and fight the user's own
 *   terminal `git` (this is the same flag VS Code's built-in Git extension
 *   uses for its background queries).
 * - `GIT_PAGER`/`PAGER=cat`: never hand output to a pager in a pipe. */
const GIT_ENV_OVERRIDES: Readonly<Record<string, string>> = {
  GIT_TERMINAL_PROMPT: "0",
  GIT_ASKPASS: "echo",
  SSH_ASKPASS: "echo",
  GIT_OPTIONAL_LOCKS: "0",
  GIT_PAGER: "cat",
  PAGER: "cat",
};

/**
 * Runs one `git` invocation to completion.
 *
 * Resolves for *any* exit code — deciding whether a non-zero exit is a
 * legitimate answer ("not a repository", "no such path in that revision")
 * or a real failure belongs to the query, not to this boundary. It rejects
 * only for problems with running `git` at all: it is missing, it timed out,
 * or the caller cancelled.
 */
export function runGit(request: GitRunRequest): Promise<GitRunResult> {
  const { cwd, args, signal } = request;
  const timeoutMs = request.timeoutMs ?? DEFAULT_GIT_TIMEOUT_MS;

  if (signal?.aborted) {
    return Promise.reject(new GitCancelledError());
  }

  return new Promise<GitRunResult>((resolve, reject) => {
    const child = spawn("git", [...args], {
      cwd,
      shell: false,
      windowsHide: true,
      // No stdin at all: every query here is read-only and reads nothing,
      // and a closed stdin is one more reason a credential prompt cannot
      // hang us.
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, ...GIT_ENV_OVERRIDES },
    });

    const stdoutChunks: Buffer[] = [];
    let stdoutBytes = 0;
    let stdoutTruncated = false;
    const stderrChunks: Buffer[] = [];
    let stderrBytes = 0;

    let settled = false;
    let timeoutHandle: ReturnType<typeof setTimeout> | undefined;

    const cleanup = (): void => {
      if (timeoutHandle !== undefined) {
        clearTimeout(timeoutHandle);
      }
      signal?.removeEventListener("abort", onAbort);
      child.stdout?.removeAllListeners();
      child.stderr?.removeAllListeners();
      child.removeAllListeners();
    };

    const settle = (fn: () => void): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      fn();
    };

    const onAbort = (): void => {
      settle(() => reject(new GitCancelledError()));
      child.kill();
    };
    signal?.addEventListener("abort", onAbort);

    timeoutHandle = setTimeout(() => {
      settle(() => reject(new GitTimeoutError(timeoutMs)));
      child.kill();
    }, timeoutMs);

    child.stdout?.on("data", (chunk: Buffer) => {
      // Past the cap the chunk is dropped but still consumed, so the child
      // never blocks on a full pipe buffer (same contract as the Rust
      // runner's `read_capped`).
      if (stdoutBytes < MAX_CAPTURED_STREAM_BYTES) {
        const take = Math.min(MAX_CAPTURED_STREAM_BYTES - stdoutBytes, chunk.length);
        stdoutChunks.push(take === chunk.length ? chunk : chunk.subarray(0, take));
        stdoutBytes += take;
        if (take < chunk.length) {
          stdoutTruncated = true;
        }
      } else {
        stdoutTruncated = true;
      }
    });
    child.stderr?.on("data", (chunk: Buffer) => {
      if (stderrBytes < MAX_CAPTURED_STREAM_BYTES) {
        const take = Math.min(MAX_CAPTURED_STREAM_BYTES - stderrBytes, chunk.length);
        stderrChunks.push(take === chunk.length ? chunk : chunk.subarray(0, take));
        stderrBytes += take;
      }
    });

    child.on("error", (err: NodeJS.ErrnoException) => {
      settle(() => {
        if (err.code === "ENOENT") {
          reject(new GitNotFoundError());
          return;
        }
        reject(
          new GitProcessError(
            `Failed to start git: ${redactSecrets(err.message)}`,
            null,
            redactSecrets(Buffer.concat(stderrChunks).toString("utf8")),
          ),
        );
      });
    });

    child.on("close", (code) => {
      settle(() => {
        resolve({
          stdout: Buffer.concat(stdoutChunks),
          // Redacted here, at the boundary, so no caller can ever get hold
          // of the raw bytes and forget.
          stderr: redactSecrets(Buffer.concat(stderrChunks).toString("utf8")),
          exitCode: code,
          truncated: stdoutTruncated,
        });
      });
    });
  });
}

/**
 * `runGit`, but a non-zero exit becomes a `GitProcessError` rejection.
 * For queries where every non-zero exit really is a failure.
 */
export async function runGitChecked(request: GitRunRequest): Promise<GitRunResult> {
  const result = await runGit(request);
  if (result.exitCode !== 0) {
    throw new GitProcessError(
      `git ${request.args[0] ?? ""} exited with code ${result.exitCode}.`,
      result.exitCode,
      result.stderr,
    );
  }
  return result;
}

/**
 * `runGitChecked`, but a non-zero exit resolves to `undefined` instead of
 * throwing — the same shape `gitsail-git`'s own `try_run` uses for the
 * queries where "git said no" is a legitimate answer rather than a fault
 * (no repository here, no such revision, no such path at that revision).
 */
export async function tryRunGit(request: GitRunRequest): Promise<GitRunResult | undefined> {
  const result = await runGit(request);
  return result.exitCode === 0 ? result : undefined;
}
