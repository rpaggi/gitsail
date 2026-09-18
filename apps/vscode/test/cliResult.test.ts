import { describe, expect, it, vi } from "vitest";

import { CliNotFoundError } from "../src/cliErrors";
import { describeCliFailure, runCli } from "../src/cliResult";
import { Envelope } from "../src/protocol";

function stubClient(run: (args: readonly string[]) => Promise<Envelope<unknown>>) {
  return { run: vi.fn(run) } as unknown as import("../src/cliClient").GitSailCliClient;
}

describe("runCli", () => {
  it("unwraps an ok envelope to its data", async () => {
    const client = stubClient(async () => ({
      status: "ok",
      schemaVersion: 1,
      requestId: "req-1",
      data: { hello: "world" },
    }));
    const result = await runCli(client, ["commit", "HEAD"]);
    expect(result).toEqual({ kind: "ok", value: { hello: "world" } });
  });

  it("surfaces a domain error distinctly from a client-level failure", async () => {
    const client = stubClient(async () => ({
      status: "error",
      schemaVersion: 1,
      requestId: "req-1",
      error: { code: "repository_not_found", message: "no such path" },
    }));
    const result = await runCli(client, ["show-file", "missing.txt"]);
    expect(result).toEqual({
      kind: "domain-error",
      error: { code: "repository_not_found", message: "no such path" },
    });
  });

  it("surfaces a thrown client-level error as cli-unavailable", async () => {
    const client = stubClient(async () => {
      throw new CliNotFoundError("/opt/gitsail/gitsail");
    });
    const result = await runCli(client, ["commit", "HEAD"]);
    expect(result.kind).toBe("cli-unavailable");
    if (result.kind === "cli-unavailable") {
      expect(result.error).toBeInstanceOf(CliNotFoundError);
    }
  });

  it("forwards run options (e.g. an AbortSignal) through to the client", async () => {
    const run = vi.fn(async () => ({
      status: "ok" as const,
      schemaVersion: 1,
      requestId: "req-1",
      data: {},
    }));
    const client = stubClient(run);
    const controller = new AbortController();
    await runCli(client, ["diff"], { signal: controller.signal });
    expect(run).toHaveBeenCalledWith(["diff"], { signal: controller.signal });
  });
});

describe("describeCliFailure", () => {
  it("appends remediation, when present, to a domain error's message", () => {
    const message = describeCliFailure({
      kind: "domain-error",
      error: { code: "parse_failure", message: "bad range", remediation: "use START-END" },
    });
    expect(message).toBe("bad range (use START-END)");
  });

  it("uses the message alone when there is no remediation", () => {
    const message = describeCliFailure({
      kind: "domain-error",
      error: { code: "internal", message: "boom" },
    });
    expect(message).toBe("boom");
  });

  it("uses the client-level error's own message", () => {
    const message = describeCliFailure({
      kind: "cli-unavailable",
      error: new CliNotFoundError("/opt/gitsail/gitsail"),
    });
    expect(message).toContain("gitsail");
  });
});
