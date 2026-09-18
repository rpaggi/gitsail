import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { forgetRecentRepository, listRecentRepositories } from "./recentRepositories";
import type { RecentRepositoryDto } from "./dto";

describe("recent repositories service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("listRecentRepositories invokes list_recent_repositories and returns its typed result", async () => {
    const entries: RecentRepositoryDto[] = [{ path: "/repo", lastOpenedUnixSeconds: 100 }];
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return entries;
    });

    const result = await listRecentRepositories();

    expect(receivedCommand).toBe("list_recent_repositories");
    expect(result).toEqual(entries);
  });

  it("forgetRecentRepository invokes forget_recent_repository with the given path", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return [];
    });

    const result = await forgetRecentRepository("/repo");

    expect(receivedCommand).toBe("forget_recent_repository");
    expect(receivedArgs).toEqual({ path: "/repo" });
    expect(result).toEqual([]);
  });
});
