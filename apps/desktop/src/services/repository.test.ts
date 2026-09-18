import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { getRepositoryStatus, openRepository } from "./repository";
import type { RepositoryDto, RepositoryStatusDto } from "./dto";

describe("repository service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("openRepository invokes open_repository with the given path and returns its typed result", async () => {
    const repo: RepositoryDto = {
      id: "repo-1",
      rootPath: "/repo",
      worktreePath: "/repo",
      isBare: false,
      headState: { state: "attached", branch: "main" },
      currentBranch: "main",
    };
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return repo;
    });

    const result = await openRepository("/repo");

    expect(receivedCommand).toBe("open_repository");
    expect(receivedArgs).toEqual({ path: "/repo" });
    expect(result).toEqual(repo);
  });

  it("getRepositoryStatus defaults to the manual reason", async () => {
    const status: RepositoryStatusDto = {
      branch: "main",
      headState: { state: "attached", branch: "main" },
      files: [],
      isClean: true,
    };
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return status;
    });

    const result = await getRepositoryStatus();

    expect(receivedCommand).toBe("get_repository_status");
    expect(receivedArgs).toEqual({ reason: "manual" });
    expect(result).toEqual(status);
  });

  it("getRepositoryStatus forwards an explicit reason (US-054 criterion 2)", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return {
        branch: "main",
        headState: { state: "attached", branch: "main" },
        files: [],
        isClean: true,
      };
    });

    await getRepositoryStatus("focus");
    expect(receivedArgs).toEqual({ reason: "focus" });

    await getRepositoryStatus("after_mutation");
    expect(receivedArgs).toEqual({ reason: "after_mutation" });
  });
});
