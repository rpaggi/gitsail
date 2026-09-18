// Pure top-level "what should the app shell show" decision (US-053 DoD:
// "documenta ... estados vazio/erro/loading"). Kept separate from any panel's
// own loading/error state (e.g. `BranchPanel.vue`'s `lastError`, which is
// about that panel's own data) — this resolves the *shell-level* state that
// decides whether `AppShell.vue` renders the full workspace (sidebar +
// graph + tabs) or one of the three states a person can hit before a
// repository is usable: still opening, no repository open yet, or the last
// open attempt failed. Extracted as a pure function (no store/DOM access)
// so it can be unit tested directly, per this project's convention of
// testing extracted pure logic rather than mounting components (no
// `@vue/test-utils` in this project).

export type ShellState =
  | { kind: "opening" }
  | { kind: "empty" }
  | { kind: "error"; message: string; remediation: string | null }
  | { kind: "ready" };

export interface ShellStateInput {
  /** `session.isOpening` — an `openRepository` call is in flight. */
  isOpening: boolean;
  /** Whether a repository is currently open (`session.repository !== null`). */
  hasRepository: boolean;
  /** `session.lastError` — the last failure, if any, from opening/refreshing. */
  lastError: { message: string; remediation?: string | null } | null;
}

/**
 * Resolves which of the four shell-level states applies, in priority order:
 * 1. `opening` — an open is in flight, regardless of prior state (a retry
 *    from the error state, or the very first open, look identical here).
 * 2. `error` — the last attempt failed and there is still no repository
 *    open to fall back to showing (an error while a repository was already
 *    open — e.g. a failed refresh — is `ready`: the workspace stays visible
 *    and that failure is that panel's own concern, not the shell's).
 * 3. `empty` — no repository open yet, no error either (the initial state).
 * 4. `ready` — a repository is open; the full workspace renders.
 */
export function resolveShellState(input: ShellStateInput): ShellState {
  if (input.isOpening) {
    return { kind: "opening" };
  }
  if (input.hasRepository) {
    return { kind: "ready" };
  }
  if (input.lastError) {
    return {
      kind: "error",
      message: input.lastError.message,
      remediation: input.lastError.remediation ?? null,
    };
  }
  return { kind: "empty" };
}
