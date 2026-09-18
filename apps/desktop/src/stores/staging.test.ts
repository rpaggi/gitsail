import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { isRelevantChange, useStagingStore } from "./staging";
import { useRepositorySessionStore } from "./session";
import { useOperationStore } from "./operation";
import type { RepositoryDto, RepositoryStatusDto } from "../services/dto";

function repo(): RepositoryDto {
  return {
    id: "/repo",
    rootPath: "/repo",
    worktreePath: "/repo",
    isBare: false,
    headState: { state: "attached", branch: "main" },
    currentBranch: "main",
  };
}

function statusWithFiles(): RepositoryStatusDto {
  return {
    branch: "main",
    headState: { state: "attached", branch: "main" },
    isClean: false,
    files: [
      {
        path: "staged.txt",
        previousPath: null,
        changeType: "modified",
        indexStatus: "modified",
        worktreeStatus: "unmodified",
      },
      {
        path: "unstaged.txt",
        previousPath: null,
        changeType: "modified",
        indexStatus: "unmodified",
        worktreeStatus: "modified",
      },
      {
        path: "new.txt",
        previousPath: null,
        changeType: "untracked",
        indexStatus: "unmodified",
        worktreeStatus: "untracked",
      },
    ],
  };
}

async function openRepoWithStatus(): Promise<void> {
  mockIPC((cmd) => {
    if (cmd === "open_repository") return repo();
    if (cmd === "get_repository_status") return statusWithFiles();
    return null;
  });
  const session = useRepositorySessionStore();
  await session.openRepository("/repo");
}

describe("isRelevantChange", () => {
  it("staged scope excludes untracked/ignored/unmodified", () => {
    expect(isRelevantChange("modified", "staged")).toBe(true);
    expect(isRelevantChange("untracked", "staged")).toBe(false);
    expect(isRelevantChange("unmodified", "staged")).toBe(false);
    expect(isRelevantChange("ignored", "staged")).toBe(false);
  });

  it("worktree scope includes untracked but excludes unmodified/ignored", () => {
    expect(isRelevantChange("untracked", "worktree")).toBe(true);
    expect(isRelevantChange("unmodified", "worktree")).toBe(false);
    expect(isRelevantChange("ignored", "worktree")).toBe(false);
  });
});

describe("staging store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("stagedFiles/unstagedFiles are derived from the session status", async () => {
    await openRepoWithStatus();
    const staging = useStagingStore();

    expect(staging.stagedFiles.map((f) => f.path)).toEqual(["staged.txt"]);
    expect(staging.unstagedFiles.map((f) => f.path)).toEqual(["unstaged.txt", "new.txt"]);
  });

  it("stageFiles is Safe risk and runs immediately", async () => {
    await openRepoWithStatus();
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "get_repository_status") return statusWithFiles();
      return null;
    });
    const staging = useStagingStore();

    await staging.stageFiles(["unstaged.txt"]);

    expect(received).toContain("stage_paths");
    expect(received).toContain("get_repository_status");
    const operation = useOperationStore();
    expect(operation.status).toBe("succeeded");
  });

  it("requestCommit is Moderate risk and preserves the typed message until confirmed", async () => {
    await openRepoWithStatus();
    const staging = useStagingStore();
    staging.message = "fix the bug";

    await staging.requestCommit();

    expect(staging.message).toBe("fix the bug");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.impact).toContain("staged file");
  });

  it("a successful commit clears the message and records the new hash", async () => {
    await openRepoWithStatus();
    mockIPC((cmd) => {
      if (cmd === "create_commit") return { hash: "c".repeat(40) };
      if (cmd === "get_repository_status") return { ...statusWithFiles(), isClean: true, files: [] };
      return null;
    });
    const staging = useStagingStore();
    staging.message = "fix the bug";

    await staging.requestCommit();
    const operation = useOperationStore();
    await operation.confirm();

    expect(staging.message).toBe("");
    expect(staging.lastCommitHash).toBe("c".repeat(40));
    expect(operation.status).toBe("succeeded");
  });

  it("a failed commit preserves the typed message and the current selection", async () => {
    await openRepoWithStatus();
    mockIPC((cmd) => {
      if (cmd === "create_commit") {
        throw { code: "invalid_repository_state", message: "nothing staged to commit" };
      }
      if (cmd === "get_repository_status") return statusWithFiles();
      return null;
    });
    const staging = useStagingStore();
    staging.message = "work in progress";

    await staging.requestCommit();
    const operation = useOperationStore();
    await operation.confirm();

    expect(staging.message).toBe("work in progress");
    expect(staging.lastCommitHash).toBeNull();
    expect(operation.status).toBe("failed");
    expect(operation.error?.message).toBe("nothing staged to commit");
    // The selection (derived from session status) is untouched too, since
    // the failed commit's `refreshAfterMutation` is never reached.
    expect(staging.stagedFiles.map((f) => f.path)).toEqual(["staged.txt"]);
  });
});
