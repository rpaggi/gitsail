// The one module in this extension allowed to spawn `gitsail-cli` (SAD §16:
// "It does not own... repository discovery rules... Git mutation
// semantics" — the extension only ever asks the Core process, never
// re-derives an answer itself). Every other module that needs Git data goes
// through `GitSailCliClient`, never `node:child_process` directly.
//
// US-069 criteria covered here:
//   1. Executes the process by argument list (never a shell string —
//      SAD §33/"command injection") and validates the envelope via
//      `parseEnvelope` (protocol.ts) before returning anything to a caller.
//   2. Never falls back to parsing Git output itself on failure — every
//      failure mode surfaces as one of `cliErrors.ts`'s typed errors.
//   3. Timeout, cancellation, and process exit all converge on one `settle`
//      path that tears down listeners/timers exactly once, so a killed or
//      finished process never leaves a stray timer or listener behind.

import { spawn } from "node:child_process";

import { CliCancelledError, CliNotFoundError, CliProcessError, CliTimeoutError } from "./cliErrors";
import { Envelope, parseEnvelope } from "./protocol";

export interface RunOptions {
  /** Working directory for the spawned process. Most commands don't need
   * this — repository selection goes through `--repo`, not `cwd` — but it
   * is accepted for callers that do. */
  cwd?: string;
  /** Abort (kill the process) if it has not settled within this many
   * milliseconds. Unset means no client-side timeout (the CLI's own
   * `--timeout` flag, passed as a normal argument when a caller wants one,
   * bounds the underlying `git` call itself; this is a separate, outer
   * guard against the CLI process itself hanging). */
  timeoutMs?: number;
  /** Aborting this signal kills the in-flight process and rejects with
   * `CliCancelledError`. */
  signal?: AbortSignal;
}

export interface CliClientOptions {
  /** Absolute path (or bare command name already resolved via PATH lookup)
   * to the `gitsail`/`gitsail.exe` executable — decided by `cliLocator.ts`.
   * This client never searches for it and never substitutes a different
   * command on failure. */
  binaryPath: string;
}

/**
 * Runs exactly one `gitsail-cli --json <args>` query per call.
 */
export class GitSailCliClient {
  constructor(private readonly options: CliClientOptions) {}

  run<T>(args: readonly string[], runOptions: RunOptions = {}): Promise<Envelope<T>> {
    const { cwd, timeoutMs, signal } = runOptions;

    if (signal?.aborted) {
      return Promise.reject(new CliCancelledError());
    }

    return new Promise<Envelope<T>>((resolve, reject) => {
      const child = spawn(this.options.binaryPath, [...args, "--json"], {
        cwd,
        shell: false,
        windowsHide: true,
      });

      let stdout = "";
      let stderr = "";
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
        settle(() => reject(new CliCancelledError()));
        child.kill();
      };
      signal?.addEventListener("abort", onAbort);

      if (timeoutMs !== undefined) {
        timeoutHandle = setTimeout(() => {
          settle(() => reject(new CliTimeoutError(timeoutMs)));
          child.kill();
        }, timeoutMs);
      }

      child.stdout?.on("data", (chunk: Buffer) => {
        stdout += chunk.toString("utf8");
      });
      child.stderr?.on("data", (chunk: Buffer) => {
        stderr += chunk.toString("utf8");
      });

      child.on("error", (err: NodeJS.ErrnoException) => {
        settle(() => {
          if (err.code === "ENOENT") {
            reject(new CliNotFoundError(this.options.binaryPath));
          } else {
            reject(
              new CliProcessError(`Failed to start the GitSail CLI: ${err.message}`, null, stderr),
            );
          }
        });
      });

      child.on("close", (code) => {
        settle(() => {
          // `gitsail-cli --json` always prints exactly one JSON line on
          // stdout, on both success and failure (`main.rs::report`); take
          // the last non-empty line defensively in case anything else ever
          // leaks onto stdout ahead of it, rather than assuming line 1.
          const line = stdout
            .split("\n")
            .map((l) => l.trim())
            .filter((l) => l.length > 0)
            .pop();
          if (line === undefined) {
            reject(
              new CliProcessError(
                `The GitSail CLI exited with code ${code} and printed no JSON output.`,
                code,
                stderr,
              ),
            );
            return;
          }
          try {
            resolve(parseEnvelope<T>(line));
          } catch (parseError) {
            reject(parseError);
          }
        });
      });
    });
  }
}
