import path from "node:path";

import { describe, expect, it } from "vitest";

import {
  CliVersionInfo,
  defaultSpawnVersionProbe,
  isVersionAtLeast,
  MINIMUM_SUPPORTED_CLI_VERSION,
  parseCliVersionOutput,
  probeCliBinary,
  resolveBinaryCommand,
} from "../src/cliLocator";

const FIXTURE = path.join(__dirname, "fixtures", "fake-cli.js");

describe("parseCliVersionOutput", () => {
  it("parses clap's plain-text --version output", () => {
    expect(parseCliVersionOutput("gitsail 0.4.2\n")).toEqual({
      raw: "0.4.2",
      major: 0,
      minor: 4,
      patch: 2,
    });
  });

  it("returns undefined for output with no recognizable version", () => {
    expect(parseCliVersionOutput("definitely not a version string")).toBeUndefined();
  });
});

describe("isVersionAtLeast", () => {
  const v = (major: number, minor: number, patch: number): CliVersionInfo => ({
    raw: `${major}.${minor}.${patch}`,
    major,
    minor,
    patch,
  });

  it("accepts an equal version", () => {
    expect(isVersionAtLeast(v(0, 4, 0), v(0, 4, 0))).toBe(true);
  });

  it("accepts a newer patch/minor/major", () => {
    expect(isVersionAtLeast(v(0, 4, 1), v(0, 4, 0))).toBe(true);
    expect(isVersionAtLeast(v(0, 5, 0), v(0, 4, 9))).toBe(true);
    expect(isVersionAtLeast(v(1, 0, 0), v(0, 9, 9))).toBe(true);
  });

  it("rejects an older version", () => {
    expect(isVersionAtLeast(v(0, 3, 9), v(0, 4, 0))).toBe(false);
  });
});

describe("resolveBinaryCommand (US-070 criterion 1 / T-204 criterion 2)", () => {
  it("defaults to PATH discovery when nothing is configured", () => {
    const resolution = resolveBinaryCommand({ isWorkspaceTrusted: true });
    expect(resolution).toMatchObject({ kind: "resolved", source: "path" });
  });

  it("uses the configured path in a trusted workspace", () => {
    const resolution = resolveBinaryCommand({
      configuredPath: "/opt/gitsail/gitsail",
      isWorkspaceTrusted: true,
    });
    expect(resolution).toEqual({
      kind: "resolved",
      command: "/opt/gitsail/gitsail",
      source: "config",
    });
  });

  it("ignores the configured path in an untrusted workspace instead of reading it", () => {
    const resolution = resolveBinaryCommand({
      configuredPath: "/opt/gitsail/gitsail",
      isWorkspaceTrusted: false,
    });
    expect(resolution).toEqual({ kind: "blocked-untrusted" });
  });

  it("treats a blank configured path the same as unconfigured", () => {
    const resolution = resolveBinaryCommand({ configuredPath: "   ", isWorkspaceTrusted: false });
    expect(resolution).toMatchObject({ kind: "resolved", source: "path" });
  });
});

describe("probeCliBinary — with a stubbed spawn (unit)", () => {
  it("reports ok for a compatible version", async () => {
    const result = await probeCliBinary(
      { isWorkspaceTrusted: true },
      MINIMUM_SUPPORTED_CLI_VERSION,
      async () => ({ stdout: "gitsail 0.0.0\n", code: 0 }),
    );
    expect(result).toMatchObject({ status: "ok" });
  });

  it("reports not-found when spawning errors", async () => {
    const result = await probeCliBinary({ isWorkspaceTrusted: true }, MINIMUM_SUPPORTED_CLI_VERSION, async () => ({
      stdout: "",
      code: null,
      error: Object.assign(new Error("spawn ENOENT"), { code: "ENOENT" }),
    }));
    expect(result.status).toBe("not-found");
  });

  it("reports incompatible for a version below the minimum", async () => {
    const result = await probeCliBinary(
      { isWorkspaceTrusted: true },
      { raw: "1.0.0", major: 1, minor: 0, patch: 0 },
      async () => ({ stdout: "gitsail 0.9.0\n", code: 0 }),
    );
    expect(result).toMatchObject({ status: "incompatible" });
  });

  it("reports unrecognized when the output has no parseable version", async () => {
    const result = await probeCliBinary({ isWorkspaceTrusted: true }, MINIMUM_SUPPORTED_CLI_VERSION, async () => ({
      stdout: "not a gitsail build\n",
      code: 1,
    }));
    expect(result.status).toBe("unrecognized");
  });

  it("reports blocked-untrusted without ever calling the spawn probe", async () => {
    let called = false;
    const result = await probeCliBinary(
      { configuredPath: "/opt/gitsail/gitsail", isWorkspaceTrusted: false },
      MINIMUM_SUPPORTED_CLI_VERSION,
      async () => {
        called = true;
        return { stdout: "gitsail 0.0.0\n", code: 0 };
      },
    );
    expect(result).toEqual({ status: "blocked-untrusted" });
    expect(called).toBe(false);
  });
});

describe("defaultSpawnVersionProbe — real process spawn (contract test)", () => {
  it("collects stdout from a real child process", async () => {
    const result = await defaultSpawnVersionProbe(process.execPath, [FIXTURE, "--version", "0.4.2"]);
    expect(result.error).toBeUndefined();
    expect(parseCliVersionOutput(result.stdout)).toEqual({
      raw: "0.4.2",
      major: 0,
      minor: 4,
      patch: 2,
    });
  });

  it("reports an ENOENT-style error for a nonexistent command", async () => {
    const result = await defaultSpawnVersionProbe("/nonexistent/gitsail-does-not-exist", ["--version"]);
    expect(result.error?.code).toBe("ENOENT");
  });
});
