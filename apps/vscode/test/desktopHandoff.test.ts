import { describe, expect, it } from "vitest";

import {
  buildDesktopHandoffArgs,
  describeDesktopLaunchFallback,
  launchDesktopForCommit,
  validateDesktopHandoffArgs,
} from "../src/desktopHandoff";

const validHash = "a".repeat(40);

describe("buildDesktopHandoffArgs (US-077 criterion 2: documented handoff mechanism)", () => {
  it("builds --repo/--commit exactly", () => {
    expect(buildDesktopHandoffArgs("/workspace/project", validHash)).toEqual([
      "--repo",
      "/workspace/project",
      "--commit",
      validHash,
    ]);
  });
});

describe("validateDesktopHandoffArgs", () => {
  it("accepts a plausible repo path and commit hash", () => {
    expect(validateDesktopHandoffArgs("/workspace/project", validHash)).toBeUndefined();
    expect(validateDesktopHandoffArgs("/workspace/project", "abc1234")).toBeUndefined();
  });

  it("rejects an empty repository path", () => {
    expect(validateDesktopHandoffArgs("  ", validHash)).toMatch(/repository/i);
  });

  it("rejects a commit-ish that does not look like a hash", () => {
    expect(validateDesktopHandoffArgs("/repo", "--not-a-hash")).toMatch(/hash/i);
    expect(validateDesktopHandoffArgs("/repo", "HEAD")).toMatch(/hash/i);
  });
});

describe("launchDesktopForCommit (US-077 DoD: correct args, missing app handled without throwing)", () => {
  it("reports not-configured without ever attempting to spawn anything", async () => {
    const spawnLaunch = async () => {
      throw new Error("must not be called");
    };
    const outcome = await launchDesktopForCommit(undefined, "/repo", validHash, spawnLaunch);
    expect(outcome).toEqual({ status: "not-configured" });
  });

  it("reports not-configured for a blank (whitespace-only) configured path", async () => {
    const outcome = await launchDesktopForCommit("   ", "/repo", validHash, async () => ({}));
    expect(outcome.status).toBe("not-configured");
  });

  it("rejects invalid arguments before spawning", async () => {
    const spawnLaunch = async () => {
      throw new Error("must not be called");
    };
    const outcome = await launchDesktopForCommit("/opt/GitSail/gitsail-desktop", "/repo", "not-a-hash", spawnLaunch);
    expect(outcome.status).toBe("invalid-arguments");
  });

  it("launches with the correct --repo/--commit arguments when configured and valid", async () => {
    const calls: { command: string; args: readonly string[] }[] = [];
    const spawnLaunch = async (command: string, args: readonly string[]) => {
      calls.push({ command, args });
      return {};
    };
    const outcome = await launchDesktopForCommit(
      "/opt/GitSail/gitsail-desktop",
      "/workspace/project",
      validHash,
      spawnLaunch,
    );
    expect(outcome).toEqual({
      status: "launched",
      command: "/opt/GitSail/gitsail-desktop",
      args: ["--repo", "/workspace/project", "--commit", validHash],
    });
    expect(calls).toEqual([
      { command: "/opt/GitSail/gitsail-desktop", args: ["--repo", "/workspace/project", "--commit", validHash] },
    ]);
  });

  it("reports not-found (never an uncaught exception) when the configured executable does not exist", async () => {
    const enoent = Object.assign(new Error("spawn ENOENT"), { code: "ENOENT" });
    const spawnLaunch = async () => ({ error: enoent as NodeJS.ErrnoException });
    const outcome = await launchDesktopForCommit("/does/not/exist", "/repo", validHash, spawnLaunch);
    expect(outcome).toEqual({ status: "not-found", command: "/does/not/exist" });
  });

  it("reports a generic failure for any other spawn error, without throwing", async () => {
    const eacces = Object.assign(new Error("spawn EACCES"), { code: "EACCES" });
    const spawnLaunch = async () => ({ error: eacces as NodeJS.ErrnoException });
    const outcome = await launchDesktopForCommit("/opt/gitsail-desktop", "/repo", validHash, spawnLaunch);
    expect(outcome.status).toBe("failed");
  });
});

describe("describeDesktopLaunchFallback (US-077 criterion 3: useful alternative, never a hard failure)", () => {
  it("mentions the setting for not-configured", () => {
    expect(describeDesktopLaunchFallback({ status: "not-configured" })).toMatch(/gitsail\.desktop\.path/);
  });

  it("mentions the configured path for not-found", () => {
    expect(describeDesktopLaunchFallback({ status: "not-found", command: "/opt/x" })).toContain("/opt/x");
  });

  it("surfaces the underlying error message for a generic failure", () => {
    expect(
      describeDesktopLaunchFallback({ status: "failed", command: "/opt/x", error: new Error("boom") }),
    ).toContain("boom");
  });

  it("surfaces the validation reason for invalid-arguments", () => {
    expect(
      describeDesktopLaunchFallback({ status: "invalid-arguments", reason: "no repository path to hand off" }),
    ).toContain("no repository path to hand off");
  });
});
