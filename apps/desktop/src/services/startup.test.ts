import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { takeStartupIntent } from "./startup";

describe("startup service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("takeStartupIntent invokes take_startup_intent and returns its result", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return { repoPath: "/repo", commitHash: "a".repeat(40) };
    });

    const intent = await takeStartupIntent();

    expect(receivedCommand).toBe("take_startup_intent");
    expect(intent).toEqual({ repoPath: "/repo", commitHash: "a".repeat(40) });
  });

  it("a plain launch reports an empty intent", async () => {
    mockIPC(() => ({ repoPath: null, commitHash: null }));

    const intent = await takeStartupIntent();

    expect(intent.repoPath).toBeNull();
    expect(intent.commitHash).toBeNull();
  });
});
