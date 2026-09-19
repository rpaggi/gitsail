// `GitResult` replaces `CliResult`, and this suite replaces
// `cliResult.test.ts`.
//
// The distinction that file existed to protect — "a domain answer the tool
// validly reported" must never be conflated with "the tool could not be
// run" — did not disappear with the CLI envelope; it moved. Each query in
// `gitClient.ts` now returns its own legitimate "no" answers as ordinary
// values (`FileContentDto`'s `missing`, discovery's `no-repository`, an
// empty history page on an unborn branch), and only genuine failures reach
// `GitResult`. Those value-shaped answers are asserted in
// `gitClient.test.ts` against real repositories; what is left to assert
// here is the wrapper itself, and the one property with real UX weight: a
// cancellation is not a failure to report.

import { describe, expect, it } from "vitest";

import { GitCancelledError, GitNotFoundError, GitTimeoutError } from "../src/git/errors";
import { describeGitFailure, isCancellation, runGitQuery } from "../src/git/result";

describe("runGitQuery", () => {
  it("wraps a successful query's value", async () => {
    const result = await runGitQuery(async () => ({ hash: "abc" }));
    expect(result).toEqual({ kind: "ok", value: { hash: "abc" } });
  });

  it("converts a thrown error into an error result rather than propagating", async () => {
    const result = await runGitQuery(async () => {
      throw new GitTimeoutError(500);
    });
    expect(result.kind).toBe("error");
    if (result.kind !== "error") return;
    expect(result.error).toBeInstanceOf(GitTimeoutError);
  });
});

describe("isCancellation", () => {
  it("recognizes a cancelled query, which must never be reported to the user", () => {
    expect(isCancellation({ kind: "error", error: new GitCancelledError() })).toBe(true);
  });

  it("does not mistake a real failure for a cancellation", () => {
    expect(isCancellation({ kind: "error", error: new GitNotFoundError() })).toBe(false);
    expect(isCancellation({ kind: "error", error: new GitTimeoutError(1) })).toBe(false);
  });
});

describe("describeGitFailure", () => {
  it("surfaces the failure's own message", () => {
    expect(describeGitFailure({ kind: "error", error: new GitTimeoutError(250) })).toContain("250ms");
  });

  it("tells a user how to fix a missing git, rather than just naming the fault", () => {
    const message = describeGitFailure({ kind: "error", error: new GitNotFoundError() });
    expect(message).toMatch(/install git/i);
    expect(message).toMatch(/PATH/);
  });
});
