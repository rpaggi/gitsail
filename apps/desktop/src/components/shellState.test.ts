import { describe, expect, it } from "vitest";

import { resolveShellState } from "./shellState";

describe("resolveShellState", () => {
  it("is 'opening' whenever an open is in flight, even with a repository already set", () => {
    expect(
      resolveShellState({ isOpening: true, hasRepository: true, lastError: null }),
    ).toEqual({ kind: "opening" });
    expect(
      resolveShellState({
        isOpening: true,
        hasRepository: false,
        lastError: { message: "boom" },
      }),
    ).toEqual({ kind: "opening" });
  });

  it("is 'ready' whenever a repository is open and no open is in flight", () => {
    expect(
      resolveShellState({ isOpening: false, hasRepository: true, lastError: null }),
    ).toEqual({ kind: "ready" });
  });

  it("prefers 'ready' over a stale error once a repository is open (a later refresh failure is the panel's own concern)", () => {
    expect(
      resolveShellState({
        isOpening: false,
        hasRepository: true,
        lastError: { message: "refresh failed" },
      }),
    ).toEqual({ kind: "ready" });
  });

  it("is 'error' with the message/remediation when the last open attempt failed and nothing is open", () => {
    expect(
      resolveShellState({
        isOpening: false,
        hasRepository: false,
        lastError: { message: "not a repository", remediation: "choose another folder" },
      }),
    ).toEqual({ kind: "error", message: "not a repository", remediation: "choose another folder" });
  });

  it("defaults remediation to null when absent", () => {
    expect(
      resolveShellState({
        isOpening: false,
        hasRepository: false,
        lastError: { message: "not a repository" },
      }),
    ).toEqual({ kind: "error", message: "not a repository", remediation: null });
  });

  it("is 'empty' with no repository, no error, and no open in flight", () => {
    expect(
      resolveShellState({ isOpening: false, hasRepository: false, lastError: null }),
    ).toEqual({ kind: "empty" });
  });
});
