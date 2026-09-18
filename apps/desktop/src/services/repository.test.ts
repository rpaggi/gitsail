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

  it("getRepositoryStatus invokes get_repository_status and returns its typed result", async () => {
    const status: RepositoryStatusDto = {
      branch: "main",
      headState: { state: "attached", branch: "main" },
      files: [],
      isClean: true,
    };
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return status;
    });

    const result = await getRepositoryStatus();

    expect(receivedCommand).toBe("get_repository_status");
    expect(result).toEqual(status);
  });
});
