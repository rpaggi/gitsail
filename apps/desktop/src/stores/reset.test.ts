import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useBranchesStore } from "./branches";
import { useOperationStore } from "./operation";
import { useResetStore } from "./reset";
import { useRepositorySessionStore } from "./session";
import type { BranchDto, RepositoryDto, RepositoryStatusDto } from "../services/dto";

function repo(rootPath: string): RepositoryDto {
  return {
    id: rootPath,
    rootPath,
    worktreePath: rootPath,
    isBare: false,
    headState: { state: "attached", branch: "main" },
    currentBranch: "main",
  };
}

function currentBranch(target: string): BranchDto {
  return {
    name: "main",
    kind: { kind: "local" },
    target,
    upstream: null,
    ahead: 0,
    behind: 0,
    isCurrent: true,
  };
}

function statusWith(fileCount: number): RepositoryStatusDto {
  return {
    branch: "main",
    headState: { state: "attached", branch: "main" },
    files: Array.from({ length: fileCount }, (_, i) => ({
      path: `f${i}.txt`,
      previousPath: null,
      changeType: "modified",
      indexStatus: "modified",
      worktreeStatus: "unmodified",
    })),
    isClean: fileCount === 0,
  };
}

describe("reset store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("is closed by default and open() opens it against the given target, defaulting to soft", () => {
    const store = useResetStore();
    expect(store.isOpen).toBe(false);

    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });

    expect(store.isOpen).toBe(true);
    expect(store.target?.shortHash).toBe("aaaaaaa");
    expect(store.mode).toBe("soft");
  });

  it("close() discards the target without running anything", () => {
    const store = useResetStore();
    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });

    store.close();

    expect(store.isOpen).toBe(false);
    expect(store.target).toBeNull();
  });

  it("setMode reassigns the chooser's highlighted mode", () => {
    const store = useResetStore();
    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });

    store.setMode("hard");

    expect(store.mode).toBe("hard");
  });

  it("predictedLossFileCount reads the live, already-loaded repository status", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      if (cmd === "get_repository_status") return statusWith(3);
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useResetStore();
    expect(store.predictedLossFileCount).toBe(0);

    await useRepositorySessionStore().openRepository("/repo");

    expect(store.predictedLossFileCount).toBe(3);
  });

  it("requestReset is a no-op without an open chooser", async () => {
    const store = useResetStore();
    await store.requestReset();
    expect(useOperationStore().status).toBe("idle");
  });

  it("requestReset is a no-op without a known current local branch", async () => {
    const store = useResetStore();
    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });

    await store.requestReset();

    expect(useOperationStore().status).toBe("idle");
    // The chooser stays open — there was nothing safe to revalidate
    // against, so this must not silently discard the pending request.
    expect(store.isOpen).toBe(true);
  });

  it("requestReset (soft) is Moderate risk, names the target/mode, and dispatches with the current branch's tip as expectedHead", async () => {
    const received: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "reset") {
        received.push(args as Record<string, unknown>);
        return null;
      }
      if (cmd === "get_repository_status") return statusWith(0);
      throw new Error(`unexpected command ${cmd}`);
    });
    useBranchesStore().branches = [currentBranch("c".repeat(40))];
    const store = useResetStore();
    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });

    await store.requestReset();
    const operation = useOperationStore();
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.targetLabel).toContain("aaaaaaa");
    expect(operation.current?.targetLabel).toContain("soft");
    expect(operation.current?.impact).toBeUndefined();
    // The chooser closes the moment this dispatches, mirroring the rebase
    // plan overlay's own "never silently reopens" convention.
    expect(store.isOpen).toBe(false);

    await operation.confirm();

    expect(received).toEqual([
      { targetRevision: "a".repeat(40), mode: "soft", expectedHead: "c".repeat(40) },
    ]);
    expect(operation.status).toBe("succeeded");
  });

  it("requestReset (hard) is Destructive risk and names the concrete predicted loss, never a generic warning", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      if (cmd === "reset") return null;
      if (cmd === "get_repository_status") return statusWith(2);
      throw new Error(`unexpected command ${cmd}`);
    });
    useBranchesStore().branches = [currentBranch("c".repeat(40))];
    await useRepositorySessionStore().openRepository("/repo");

    const store = useResetStore();
    store.open({ hash: "a".repeat(40), shortHash: "aaaaaaa" });
    store.setMode("hard");

    await store.requestReset();

    const operation = useOperationStore();
    expect(operation.current?.risk).toBe("destructive");
    expect(operation.current?.impact).toContain("2 uncommitted changes");
    expect(operation.current?.impact).toContain("permanently discarded");
  });
});
