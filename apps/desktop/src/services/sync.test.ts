import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { fetch, listRemotes, pull, push, resolveSyncTarget } from "./sync";
import type { PullResultDto, RemoteDto, SyncTargetDto } from "./dto";

function sampleRemotes(): RemoteDto[] {
  return [{ name: "origin", fetchUrl: "https://example.com/repo.git", pushUrl: "https://example.com/repo.git" }];
}

describe("sync service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("listRemotes invokes list_remotes and returns every configured remote", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return sampleRemotes();
    });

    const remotes = await listRemotes();

    expect(receivedCommand).toBe("list_remotes");
    expect(remotes).toHaveLength(1);
    expect(remotes[0].name).toBe("origin");
  });

  it("resolveSyncTarget invokes resolve_sync_target and returns the resolved remote/branch", async () => {
    let receivedCommand = "";
    const target: SyncTargetDto = { remote: "origin", branch: "main" };
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return target;
    });

    const result = await resolveSyncTarget();

    expect(receivedCommand).toBe("resolve_sync_target");
    expect(result).toEqual(target);
  });

  it("fetch invokes the fetch command with no arguments", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
    });

    const result = await fetch();

    expect(receivedCommand).toBe("fetch");
    expect(receivedArgs).toEqual({});
    expect(result.remote).toBe("origin");
  });

  it("pull invokes the pull command and returns the outcome dto", async () => {
    let receivedCommand = "";
    const outcome: PullResultDto = {
      remote: "origin",
      branch: "main",
      outcome: { outcome: "alreadyUpToDate" },
    };
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return outcome;
    });

    const result = await pull();

    expect(receivedCommand).toBe("pull");
    expect(result).toEqual(outcome);
  });

  it("push invokes the push command", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
    });

    const result = await push();

    expect(receivedCommand).toBe("push");
    expect(result.branch).toBe("main");
  });
});
