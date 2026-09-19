// Failure hierarchy for running `git` and trusting its output.
//
// Replaces the pre-ADR-025 `cliErrors.ts`, which described problems with
// the `gitsail` CLI binary. The distinction that file drew — "a problem
// running the tool at all" versus "a normal domain answer the tool validly
// reported" — still matters and is preserved here, it just no longer has a
// JSON envelope to read the second half out of: with `git` there is no
// envelope, so each query decides for itself which non-zero exits are
// legitimate answers ("this path has no repository", "this file did not
// exist at this revision") and which are genuine failures.
//
// Every message in this file is safe to show a user: any text derived from
// `git`'s own stderr passes through `redactSecrets` (`redact.ts`) before it
// reaches a constructor here.

/** `git` itself could not be spawned (ENOENT or equivalent): not installed,
 * or not on the PATH the extension host inherited. */
export class GitNotFoundError extends Error {
  constructor() {
    super(
      'Could not run "git". GitSail reads repository data with the Git you already have installed — install Git, or make sure it is on the PATH that VS Code sees, then reload the window.',
    );
    this.name = "GitNotFoundError";
  }
}

/** `git` ran but exited non-zero for a query that had no legitimate
 * non-zero answer. `stderr` is always already redacted. */
export class GitProcessError extends Error {
  constructor(
    message: string,
    readonly exitCode: number | null,
    /** Redacted stderr — see `redact.ts`. Never raw. */
    readonly stderr: string,
  ) {
    super(message);
    this.name = "GitProcessError";
  }
}

/** The query was killed after exceeding its allotted time. */
export class GitTimeoutError extends Error {
  constructor(readonly timeoutMs: number) {
    super(`The git query did not respond within ${timeoutMs}ms and was terminated.`);
    this.name = "GitTimeoutError";
  }
}

/** The caller cancelled the query (its `AbortSignal` fired) before it
 * completed — e.g. the cursor moved and a debounced blame query was
 * abandoned. Not a failure to report to the user: whoever cancelled it
 * already knows. */
export class GitCancelledError extends Error {
  constructor() {
    super("The git query was cancelled.");
    this.name = "GitCancelledError";
  }
}

/** `git` succeeded but printed something this extension could not parse.
 * Deliberately fatal to the call rather than guessed at: a half-understood
 * blame or diff would put wrong authorship on screen, which is worse than
 * showing nothing (the same reasoning `protocol.ts`'s strict envelope
 * parsing used to encode for the CLI). */
export class GitParseError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "GitParseError";
  }
}

export type GitError =
  | GitNotFoundError
  | GitProcessError
  | GitTimeoutError
  | GitCancelledError
  | GitParseError;
