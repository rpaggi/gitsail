import path from "node:path";

import { describe, expect, it } from "vitest";

import { GitSailCliClient } from "../src/cliClient";
import { CliCancelledError, CliNotFoundError, CliProcessError, CliTimeoutError } from "../src/cliErrors";
import { EnvelopeParseError, UnsupportedSchemaVersionError } from "../src/protocol";

const FIXTURE = path.join(__dirname, "fixtures", "fake-cli.js");

// The client is generic about what it spawns; using the current Node
// executable as "the binary" and this fixture script as its first argument
// lets these contract tests run identically on Linux/macOS/Windows without
// depending on a real `gitsail` build or a shell (SAD §35's CI matrix).
function client(): GitSailCliClient {
  return new GitSailCliClient({ binaryPath: process.execPath });
}

describe("GitSailCliClient — contract tests (US-069)", () => {
  it("returns a valid ok envelope's data untouched", async () => {
    const envelope = await client().run<{ currentBranch: string | null }>([FIXTURE, "ok"]);
    expect(envelope.status).toBe("ok");
    if (envelope.status === "ok") {
      expect(envelope.data.currentBranch).toBe("main");
    }
  });

  it("returns a valid error envelope as data, not as a thrown error", async () => {
    const envelope = await client().run([FIXTURE, "repository-not-found"]);
    expect(envelope.status).toBe("error");
    if (envelope.status === "error") {
      expect(envelope.error.code).toBe("repository_not_found");
    }
  });

  it("rejects with UnsupportedSchemaVersionError for an incompatible schema, never guessing at the shape", async () => {
    await expect(client().run([FIXTURE, "bad-schema"])).rejects.toBeInstanceOf(
      UnsupportedSchemaVersionError,
    );
  });

  it("rejects with EnvelopeParseError for output that is not JSON", async () => {
    await expect(client().run([FIXTURE, "malformed"])).rejects.toBeInstanceOf(EnvelopeParseError);
  });

  it("rejects with CliProcessError when the process exits without printing anything", async () => {
    await expect(client().run([FIXTURE, "no-output"])).rejects.toBeInstanceOf(CliProcessError);
  });

  it("rejects with CliNotFoundError when the binary path does not exist", async () => {
    const missing = new GitSailCliClient({ binaryPath: "/nonexistent/gitsail-does-not-exist" });
    await expect(missing.run(["open"])).rejects.toBeInstanceOf(CliNotFoundError);
  });

  it("rejects with CliTimeoutError and stops waiting once the timeout elapses", async () => {
    await expect(
      client().run([FIXTURE, "hang"], { timeoutMs: 50 }),
    ).rejects.toBeInstanceOf(CliTimeoutError);
  });

  it("rejects with CliCancelledError when the signal aborts mid-flight, and cleans up so nothing hangs the test process", async () => {
    const controller = new AbortController();
    const pending = client().run([FIXTURE, "hang"], { signal: controller.signal });
    controller.abort();
    await expect(pending).rejects.toBeInstanceOf(CliCancelledError);
  });

  it("rejects immediately with CliCancelledError for an already-aborted signal, without spawning anything", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      client().run([FIXTURE, "ok"], { signal: controller.signal }),
    ).rejects.toBeInstanceOf(CliCancelledError);
  });
});
