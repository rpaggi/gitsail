// Shared outcome wrapper for every git query, replacing the
// pre-ADR-025 `cliResult.ts`.
//
// What changed, and why it is simpler: `cliResult.ts` had to distinguish a
// well-formed *domain* error the CLI reported inside its JSON envelope
// ("this file did not exist at this revision") from a client-level failure
// ("the binary is missing"), because both arrived through the same call.
// Running `git` directly, there is no envelope and therefore no second
// error channel — so the queries in `gitClient.ts` each decide for
// themselves which exit statuses are legitimate answers and return those as
// ordinary values (`FileContentDto`'s `missing` kind, discovery's
// `no-repository` outcome), and everything that reaches here is a genuine
// failure.
//
// The one property that does carry over unchanged: `GitCancelledError` is
// never a failure worth showing a user — whoever cancelled the query
// already knows — so callers must keep checking for it rather than
// reporting every non-`ok` result.

import { GitCancelledError } from "./errors";

export type GitResult<T> =
  | { kind: "ok"; value: T }
  | { kind: "error"; error: Error };

export type GitFailure = { kind: "error"; error: Error };

/** Runs a query, converting a thrown `GitError` into an `error` result so
 * call sites can branch instead of wrapping every call in try/catch. */
export async function runGitQuery<T>(query: () => Promise<T>): Promise<GitResult<T>> {
  try {
    return { kind: "ok", value: await query() };
  } catch (error) {
    return { kind: "error", error: error as Error };
  }
}

/** True when a failure is just "the caller cancelled" — never worth a
 * message. */
export function isCancellation(result: GitFailure): boolean {
  return result.error instanceof GitCancelledError;
}

/** A user-facing message for a failed query, so every command handler shows
 * a consistent shape instead of improvising one per call site. Every
 * message reaching here was already built from redacted text (see
 * `process.ts`), so this never needs to redact again — but it also never
 * appends raw stderr, so a credential-shaped fragment has no second route
 * to the screen. */
export function describeGitFailure(result: GitFailure): string {
  return result.error.message;
}
