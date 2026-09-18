// Shared envelope-handling helper for every EPIC-15 CLI query
// (commit/commit-diff/line-history/show-file/file-history), generalizing
// the same three-way outcome `repositoryContext.ts` already established
// for `gitsail open` (US-069 criterion 2): a well-formed domain error
// (`ErrorPayload`) is never conflated with a client-level failure (missing
// binary, bad schema, timeout, ...) — callers that care about a specific
// domain error code (e.g. "this file did not exist at this revision") can
// inspect `error.code` themselves rather than this module guessing which
// domain errors are "expected" for every possible query, the way
// `repositoryContext.ts` can for the one query it owns.

import { GitSailCliClient, RunOptions } from "./cliClient";
import { ErrorPayload } from "./protocol";

export type CliResult<T> =
  | { kind: "ok"; value: T }
  | { kind: "domain-error"; error: ErrorPayload }
  /** A client-level problem running/trusting the CLI itself — never a
   * domain error the CLI validly reported (see `cliErrors.ts`). */
  | { kind: "cli-unavailable"; error: Error };

export async function runCli<T>(
  client: GitSailCliClient,
  args: readonly string[],
  options?: RunOptions,
): Promise<CliResult<T>> {
  try {
    const envelope = await client.run<T>(args, options);
    if (envelope.status === "ok") {
      return { kind: "ok", value: envelope.data };
    }
    return { kind: "domain-error", error: envelope.error };
  } catch (error) {
    return { kind: "cli-unavailable", error: error as Error };
  }
}

/** A user-facing message for any non-`"ok"` `CliResult`, so every command
 * handler shows a consistent message shape instead of improvising its own
 * per call site. */
export function describeCliFailure(result: { kind: "domain-error"; error: ErrorPayload } | { kind: "cli-unavailable"; error: Error }): string {
  if (result.kind === "domain-error") {
    return result.error.remediation
      ? `${result.error.message} (${result.error.remediation})`
      : result.error.message;
  }
  return result.error.message;
}
