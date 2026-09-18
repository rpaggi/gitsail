// Handoff to GitSail Desktop (T-210/US-077): launches the Desktop app
// pointed at the same repository and commit currently being inspected in
// VS Code.
//
// Honest limitation, documented rather than hidden (US-077's own note: "não
// exigindo um daemon rodando em background" and "Desktop ausente... sem
// falhar o restante da extensão"): `apps/desktop` does not parse any
// command-line arguments today (confirmed by reading
// `apps/desktop/src-tauri/src/{main,lib}.rs` — `run()` takes no arguments
// and there is no CLI-parsing crate wired in). This module still
// implements and validates the VS Code side of the contract (`--repo
// <path> --commit <hash>`) exactly as specified, so it activates the
// moment EPIC-12 teaches the Desktop side to read them — until then, the
// Desktop process launches but simply opens to whatever it always opens to,
// ignoring the extra arguments — a real, acknowledged gap on the Desktop
// side, not a VS Code-side failure (see this extension's README).
//
// No daemon/IPC: each handoff is a fresh process launch, detached from this
// extension host, matching "sem exigir um daemon rodando em background".

import { spawn } from "node:child_process";

export function buildDesktopHandoffArgs(repoRoot: string, commitHash: string): string[] {
  return ["--repo", repoRoot, "--commit", commitHash];
}

/** A conservative, allow-list validation of the two values that end up as
 * process arguments (US-077 DoD: "valida os argumentos"). `spawn` already
 * passes them as an argv array, never a shell string, so there is no
 * injection risk here as such — this check exists to catch a caller bug
 * (e.g. an empty/garbled repo path, or a commit-ish that clearly is not a
 * hash, such as a stray CLI flag) before ever launching a second process. */
export function validateDesktopHandoffArgs(repoRoot: string, commitHash: string): string | undefined {
  if (repoRoot.trim().length === 0) {
    return "no repository path to hand off";
  }
  if (!/^[0-9a-f]{7,40}$/i.test(commitHash)) {
    return `"${commitHash}" does not look like a commit hash`;
  }
  return undefined;
}

export type DesktopLaunchOutcome =
  | { status: "not-configured" }
  | { status: "invalid-arguments"; reason: string }
  | { status: "launched"; command: string; args: string[] }
  | { status: "not-found"; command: string }
  | { status: "failed"; command: string; error: Error };

export interface SpawnDesktopLaunchResult {
  error?: NodeJS.ErrnoException;
}

/** Injected so tests never spawn a real process (mirrors
 * `cliLocator.ts`'s `SpawnVersionProbe` pattern). Resolves once the child
 * process either reports a spawn error or is confirmed spawned — this
 * function's job ends there; it never waits for the Desktop process to
 * exit (US-077 criterion 2: no background daemon, but also no blocking on
 * a long-lived GUI app). */
export type SpawnDesktopLaunch = (
  command: string,
  args: readonly string[],
) => Promise<SpawnDesktopLaunchResult>;

export const defaultSpawnDesktopLaunch: SpawnDesktopLaunch = (command, args) =>
  new Promise((resolve) => {
    try {
      const child = spawn(command, args as string[], {
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      });
      child.once("error", (error: NodeJS.ErrnoException) => resolve({ error }));
      child.once("spawn", () => {
        // Detached + unref'd: this extension never waits on, and never
        // keeps the extension host process alive for, the Desktop app.
        child.unref();
        resolve({});
      });
    } catch (error) {
      resolve({ error: error as NodeJS.ErrnoException });
    }
  });

/**
 * Attempts to open GitSail Desktop at `repoRoot`, selecting `commitHash`
 * (US-077 criterion 1). `configuredPath` is the `gitsail.desktop.path`
 * setting's raw value — there is no PATH-based discovery for a GUI app
 * bundle the way `cliLocator.ts` discovers the CLI, so an unset path is
 * "not configured", not "not found" (criterion 3: a clearly different,
 * actionable state).
 */
export async function launchDesktopForCommit(
  configuredPath: string | undefined,
  repoRoot: string,
  commitHash: string,
  spawnLaunch: SpawnDesktopLaunch = defaultSpawnDesktopLaunch,
): Promise<DesktopLaunchOutcome> {
  const command = configuredPath?.trim();
  if (!command) {
    return { status: "not-configured" };
  }
  const invalidReason = validateDesktopHandoffArgs(repoRoot, commitHash);
  if (invalidReason) {
    return { status: "invalid-arguments", reason: invalidReason };
  }

  const args = buildDesktopHandoffArgs(repoRoot, commitHash);
  const { error } = await spawnLaunch(command, args);
  if (!error) {
    return { status: "launched", command, args };
  }
  if (error.code === "ENOENT") {
    return { status: "not-found", command };
  }
  return { status: "failed", command, error };
}

/** A user-facing message for every non-`"launched"` outcome (US-077
 * criterion 3: "apresenta uma alternativa útil... sem falhar o restante da
 * extensão") — the caller (`extension.ts`) pairs this with a "Copy commit
 * hash" action so there is always something useful to do even when the
 * handoff itself did not happen. */
export function describeDesktopLaunchFallback(outcome: Exclude<DesktopLaunchOutcome, { status: "launched" }>): string {
  switch (outcome.status) {
    case "not-configured":
      return 'GitSail Desktop is not configured. Set "gitsail.desktop.path" to its executable to open commits there directly, or copy the commit hash instead.';
    case "invalid-arguments":
      return `Could not hand off to GitSail Desktop: ${outcome.reason}.`;
    case "not-found":
      return `Could not find GitSail Desktop at "${outcome.command}". Check the "gitsail.desktop.path" setting, or copy the commit hash instead.`;
    case "failed":
      return `Could not launch GitSail Desktop: ${outcome.error.message}`;
  }
}
