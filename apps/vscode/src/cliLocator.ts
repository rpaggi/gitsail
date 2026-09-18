// Locating and verifying the `gitsail-cli` binary (US-070).
//
// Distribution decision (US-070 criterion 2 — see also
// `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`, ADR-015, and this
// extension's README "Binary distribution" section for the full writeup):
// for v0.4 this extension does NOT bundle a per-platform `gitsail-cli`
// binary. EPIC-25 (Distribution & Updates) has not shipped, so there is no
// release pipeline yet that builds, signs, or notarizes per-OS/arch
// binaries GitSail could safely embed in the `.vsix`. Bundling an unsigned,
// hand-built binary now would fail US-070 criterion 3 ("origem/versão são
// verificáveis; não há download/execução silenciosa de arquivo não
// confiável") in spirit even if not literally downloaded at runtime — the
// extension would be silently trusting a binary of unknown provenance. The
// v0.4 requirement instead is: the user installs `gitsail` themselves (same
// binary EPIC-08 already ships for the CLI/TUI) and either puts it on PATH
// or points `gitsail.binaryPath` at it. This is revisited once EPIC-25
// defines a real signed-artifact pipeline this extension can embed.

import { spawn } from "node:child_process";

/** A parsed `gitsail --version` result, e.g. `gitsail 0.4.2` -> {0,4,2}. */
export interface CliVersionInfo {
  raw: string;
  major: number;
  minor: number;
  patch: number;
}

/**
 * The minimum `gitsail-cli` version this extension is known to work with.
 * `gitsail-cli` itself is still versioned `0.0.0` workspace-wide (no crate
 * has adopted real semver yet — see every crate's Cargo.toml), so this
 * constant is a placeholder that always passes today; it exists so the
 * compatibility *mechanism* (US-070 criterion 3) is in place and exercised
 * by tests, ready to tighten the moment `gitsail-cli` starts publishing
 * real version numbers, instead of bolting the check on retroactively.
 */
export const MINIMUM_SUPPORTED_CLI_VERSION: CliVersionInfo = {
  raw: "0.0.0",
  major: 0,
  minor: 0,
  patch: 0,
};

/** `gitsail`'s own executable name, platform-appropriate, used for PATH
 * discovery when no explicit `gitsail.binaryPath` applies. */
export const DEFAULT_PATH_COMMAND = process.platform === "win32" ? "gitsail.exe" : "gitsail";

/**
 * Parses clap's plain-text `--version` output (`gitsail-cli`'s
 * `#[command(name = "gitsail", version, ...)]`), e.g. `"gitsail 0.4.2\n"`.
 * Returns `undefined` for output that does not contain a recognizable
 * `x.y.z` version — treated by callers as "not actually gitsail" rather
 * than guessed at.
 */
export function parseCliVersionOutput(output: string): CliVersionInfo | undefined {
  const match = output.trim().match(/(\d+)\.(\d+)\.(\d+)/);
  if (!match) {
    return undefined;
  }
  return {
    raw: match[0],
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
  };
}

/** Simple major.minor.patch comparison — sufficient for the CLI's current
 * `0.0.0` versioning; revisit if pre-release/build metadata is ever needed. */
export function isVersionAtLeast(version: CliVersionInfo, minimum: CliVersionInfo): boolean {
  if (version.major !== minimum.major) {
    return version.major > minimum.major;
  }
  if (version.minor !== minimum.minor) {
    return version.minor > minimum.minor;
  }
  return version.patch >= minimum.patch;
}

export interface VersionProbeResult {
  stdout: string;
  code: number | null;
  error?: NodeJS.ErrnoException;
}

/** Spawns `command --version` and collects its stdout. Injected as a
 * parameter (rather than called directly) so tests can substitute a fake
 * executable without touching PATH or installing a real `gitsail`. */
export type SpawnVersionProbe = (command: string, args: readonly string[]) => Promise<VersionProbeResult>;

export const defaultSpawnVersionProbe: SpawnVersionProbe = (command, args) =>
  new Promise((resolve) => {
    const child = spawn(command, args as string[], { shell: false, windowsHide: true });
    let stdout = "";
    child.stdout?.on("data", (chunk: Buffer) => {
      stdout += chunk.toString("utf8");
    });
    child.on("error", (error: NodeJS.ErrnoException) => resolve({ stdout, code: null, error }));
    child.on("close", (code) => resolve({ stdout, code }));
  });

export interface BinaryConfig {
  /** The raw `gitsail.binaryPath` setting value, if any (empty/undefined
   * means "not configured"). */
  configuredPath?: string;
  isWorkspaceTrusted: boolean;
}

export type BinaryResolution =
  | { kind: "resolved"; command: string; source: "config" | "path" }
  /** A `gitsail.binaryPath` is configured, but the workspace is untrusted
   * (T-204 criterion 2) — never read until trust is granted. */
  | { kind: "blocked-untrusted" };

/**
 * Decides *which* executable name/path to try (US-070 criterion 1),
 * without itself checking whether it actually exists or runs — only
 * spawning it (in `probeCliBinary`) can tell us that, and doing it here too
 * would duplicate the one real check.
 *
 * Workspace trust gates the *configured* path (T-204 criterion 2): an
 * untrusted workspace's `gitsail.binaryPath` may come from
 * `.vscode/settings.json` committed by someone else and could point at an
 * arbitrary executable, so it is never read before the workspace is
 * trusted. PATH-based discovery is not gated the same way — the PATH
 * environment variable itself is not workspace-controlled content, it is
 * the user's own machine configuration.
 */
export function resolveBinaryCommand(config: BinaryConfig): BinaryResolution {
  const configured = config.configuredPath?.trim();
  if (configured && configured.length > 0) {
    if (!config.isWorkspaceTrusted) {
      return { kind: "blocked-untrusted" };
    }
    return { kind: "resolved", command: configured, source: "config" };
  }
  return { kind: "resolved", command: DEFAULT_PATH_COMMAND, source: "path" };
}

export type CliProbeResult =
  | { status: "ok"; command: string; source: "config" | "path"; version: CliVersionInfo }
  | { status: "blocked-untrusted" }
  | { status: "not-found"; command: string }
  | { status: "incompatible"; command: string; version: CliVersionInfo }
  | { status: "unrecognized"; command: string };

/**
 * Resolves and verifies the `gitsail-cli` binary in one call: which
 * command to try, whether it can even be spawned, whether its
 * `--version` output is recognizable, and whether that version meets
 * `minimum` (US-070 criteria 1 and 3). Never downloads or executes
 * anything beyond a `--version` probe of a path the caller/PATH already
 * named — see the module doc comment for the v0.4 distribution decision
 * this deliberately upholds.
 */
export async function probeCliBinary(
  config: BinaryConfig,
  minimum: CliVersionInfo = MINIMUM_SUPPORTED_CLI_VERSION,
  spawnProbe: SpawnVersionProbe = defaultSpawnVersionProbe,
): Promise<CliProbeResult> {
  const resolution = resolveBinaryCommand(config);
  if (resolution.kind === "blocked-untrusted") {
    return { status: "blocked-untrusted" };
  }

  const { command, source } = resolution;
  const result = await spawnProbe(command, ["--version"]);
  if (result.error) {
    return { status: "not-found", command };
  }

  const version = parseCliVersionOutput(result.stdout);
  if (!version) {
    return { status: "unrecognized", command };
  }
  if (!isVersionAtLeast(version, minimum)) {
    return { status: "incompatible", command, version };
  }
  return { status: "ok", command, source, version };
}
