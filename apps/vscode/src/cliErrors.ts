// Client-level failure hierarchy for running `gitsail-cli` and trusting its
// output (US-069 criterion 2). Every class here is a problem with *running
// the CLI at all* or with *trusting what it printed* — as opposed to a
// normal domain `ErrorPayload` the CLI itself validly reported (e.g. "not a
// Git repository"). Callers must tell the two apart: a missing binary needs
// a persistent, actionable notice, while "not a repository" is a routine,
// silent, per-folder result (US-068 criterion 3).

/** `gitsail`/`gitsail.exe` could not be spawned at the given path (ENOENT or
 * equivalent) — missing install, wrong PATH, or a stale configured path. */
export class CliNotFoundError extends Error {
  constructor(readonly binaryPath: string) {
    super(
      `Could not run the GitSail CLI ("${binaryPath}"). Install gitsail so it is on your PATH, or set the "gitsail.binaryPath" setting to its full path.`,
    );
    this.name = "CliNotFoundError";
  }
}

/** The binary ran, but reports an older version than this extension
 * requires (US-070 criterion 3). */
export class CliIncompatibleVersionError extends Error {
  constructor(
    readonly detectedVersion: string,
    readonly minimumVersion: string,
  ) {
    super(
      `The GitSail CLI at version ${detectedVersion} is older than the minimum version this extension supports (${minimumVersion}). Update gitsail, or point "gitsail.binaryPath" at a compatible build.`,
    );
    this.name = "CliIncompatibleVersionError";
  }
}

/** The binary ran, but its `--version` output was not recognizable at all —
 * most likely a different program entirely at the configured/discovered
 * path, not a gitsail build this extension can safely reason about. */
export class CliUnrecognizedBinaryError extends Error {
  constructor(readonly command: string) {
    super(
      `"${command}" did not report a recognizable GitSail CLI version. Check that "gitsail.binaryPath" (or your PATH) points at the gitsail executable, not a different program.`,
    );
    this.name = "CliUnrecognizedBinaryError";
  }
}

/** The process ran but produced no parseable JSON on stdout (crash, wrong
 * binary, or an unexpected non-zero exit before it could print anything). */
export class CliProcessError extends Error {
  constructor(
    message: string,
    readonly exitCode: number | null,
    readonly stderr: string,
  ) {
    super(message);
    this.name = "CliProcessError";
  }
}

/** The query was killed after exceeding its allotted time. */
export class CliTimeoutError extends Error {
  constructor(readonly timeoutMs: number) {
    super(`The GitSail CLI did not respond within ${timeoutMs}ms and was terminated.`);
    this.name = "CliTimeoutError";
  }
}

/** The caller cancelled the query (its `AbortSignal` fired) before it
 * completed. Not a failure to report to the user — callers that started the
 * query intentionally already know it was cancelled. */
export class CliCancelledError extends Error {
  constructor() {
    super("The GitSail CLI query was cancelled.");
    this.name = "CliCancelledError";
  }
}

/** Any client-level CLI problem this module defines (never a domain
 * `ErrorPayload` — see the module doc comment above). */
export type CliClientError =
  | CliNotFoundError
  | CliIncompatibleVersionError
  | CliUnrecognizedBinaryError
  | CliProcessError
  | CliTimeoutError
  | CliCancelledError;
