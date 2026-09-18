import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import {
  connectForgeAccount,
  disconnectForgeAccount,
  forgeConnectionStatus,
  getForgeLink,
  openForgeLink,
} from "./forge";
import type { ForgeAccountDto } from "./dto";

function githubAccount(): ForgeAccountDto {
  return { kind: "gitHub", host: "github.com" };
}

describe("forge service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("getForgeLink invokes get_forge_link with the given target and returns the resolved url", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return "https://github.com/org/repo";
    });

    const link = await getForgeLink({ kind: "repository" });

    expect(receivedCommand).toBe("get_forge_link");
    expect(receivedArgs).toEqual({ target: { kind: "repository" } });
    expect(link).toBe("https://github.com/org/repo");
  });

  it("getForgeLink returns null when no remote resolves to a known forge (never a rejected promise)", async () => {
    mockIPC(() => null);

    const link = await getForgeLink({ kind: "branch", name: "main" });

    expect(link).toBeNull();
  });

  it("openForgeLink invokes open_forge_link with the given target", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return true;
    });

    const opened = await openForgeLink({ kind: "commit", hash: "deadbeef" });

    expect(receivedCommand).toBe("open_forge_link");
    expect(receivedArgs).toEqual({ target: { kind: "commit", hash: "deadbeef" } });
    expect(opened).toBe(true);
  });

  it("forgeConnectionStatus invokes forge_connection_status with the given account", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return "notConnected";
    });

    const status = await forgeConnectionStatus(githubAccount());

    expect(receivedCommand).toBe("forge_connection_status");
    expect(receivedArgs).toEqual({ account: githubAccount() });
    expect(status).toBe("notConnected");
  });

  it("connectForgeAccount invokes connect_forge_account with the account and token", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await connectForgeAccount(githubAccount(), "sentinel-fake-token");

    expect(receivedCommand).toBe("connect_forge_account");
    expect(receivedArgs).toEqual({ account: githubAccount(), token: "sentinel-fake-token" });
  });

  it("disconnectForgeAccount invokes disconnect_forge_account with the account", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await disconnectForgeAccount(githubAccount());

    expect(receivedCommand).toBe("disconnect_forge_account");
    expect(receivedArgs).toEqual({ account: githubAccount() });
  });
});
