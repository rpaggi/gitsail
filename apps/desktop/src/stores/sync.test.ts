import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useSyncStore } from "./sync";
import { useOperationStore } from "./operation";
import type { RemoteDto, SyncTargetDto } from "../services/dto";

function sampleRemotes(): RemoteDto[] {
  return [{ name: "origin", fetchUrl: "https://example.com/repo.git", pushUrl: "https://example.com/repo.git" }];
}

function statusResponse() {
  return { branch: "main", headState: { state: "attached", branch: "main" }, files: [], isClean: true };
}

describe("sync store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loadRemotes populates the remote list", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_remotes") return sampleRemotes();
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.loadRemotes();

    expect(store.remotes).toHaveLength(1);
    expect(store.remotes[0].name).toBe("origin");
  });

  it("refreshTarget records the resolved remote and branch", async () => {
    const target: SyncTargetDto = { remote: "origin", branch: "main" };
    mockIPC((cmd) => {
      if (cmd === "resolve_sync_target") return target;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    const result = await store.refreshTarget();

    expect(result).toEqual(target);
    expect(store.resolvedTarget).toEqual(target);
    expect(store.resolveError).toBeNull();
  });

  it("refreshTarget records a clear error instead of a resolved target when resolution fails", async () => {
    mockIPC(() => {
      throw {
        code: "invalid_repository_state",
        message: "the current branch has no upstream and multiple remotes are configured — cannot determine which to use",
      };
    });
    const store = useSyncStore();

    const result = await store.refreshTarget();

    expect(result).toBeNull();
    expect(store.resolvedTarget).toBeNull();
    expect(store.resolveError?.code).toBe("invalid_repository_state");
  });

  it("requestFetch is Safe risk; it resolves the target and dispatches without confirmation", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "resolve_sync_target") return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
      if (cmd === "fetch") return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.requestFetch();

    expect(received).toEqual(["resolve_sync_target", "fetch"]);
    const operation = useOperationStore();
    expect(operation.status).toBe("succeeded");
    expect(operation.current?.promptLabel).toBe("Fetch from remote 'origin'");
    expect(store.lastFetchResult?.remote).toBe("origin");
  });

  it("requestFetch dispatches nothing when the sync target cannot be resolved", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "resolve_sync_target") {
        throw { code: "invalid_repository_state", message: "no remote is configured" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.requestFetch();

    expect(received).toEqual(["resolve_sync_target"]);
    expect(store.resolveError?.message).toBe("no remote is configured");
    const operation = useOperationStore();
    expect(operation.status).toBe("idle");
  });

  it("requestPull is Moderate risk and waits for explicit confirmation before pulling", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "resolve_sync_target") return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
      if (cmd === "pull") {
        return { remote: "origin", branch: "main", outcome: { outcome: "alreadyUpToDate" } };
      }
      if (cmd === "get_repository_status") return statusResponse();
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.requestPull();

    expect(received).toEqual(["resolve_sync_target"]);
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.promptLabel).toBe("Pull branch 'main' from remote 'origin' (fast-forward only)");

    await operation.confirm();

    expect(received).toContain("pull");
    expect(store.lastPullResult?.outcome).toEqual({ outcome: "alreadyUpToDate" });
    expect(operation.status).toBe("succeeded");
  });

  // -- T-243/US-101: repository forge link --------------------------------

  it("refreshForgeLink records the resolved repository link", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "get_forge_link") {
        receivedArgs = args;
        return "https://github.com/org/repo";
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.refreshForgeLink();

    expect(receivedArgs).toEqual({ target: { kind: "repository" } });
    expect(store.forgeLink).toBe("https://github.com/org/repo");
  });

  it("refreshForgeLink leaves the link null when no remote resolves to a known forge", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_forge_link") return null;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.refreshForgeLink();

    expect(store.forgeLink).toBeNull();
  });

  it("openRepositoryForgeLink is a no-op when no forge link was resolved", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.openRepositoryForgeLink();

    expect(received).toEqual([]);
  });

  it("openRepositoryForgeLink opens the resolved repository link", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "get_forge_link") return "https://github.com/org/repo";
      if (cmd === "open_forge_link") return true;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();
    await store.refreshForgeLink();

    await store.openRepositoryForgeLink();

    expect(received).toEqual(["get_forge_link", "open_forge_link"]);
  });

  it("requestPush is Moderate risk and reports a rejected non-fast-forward as a failed operation", async () => {
    mockIPC((cmd) => {
      if (cmd === "resolve_sync_target") return { remote: "origin", branch: "main" } satisfies SyncTargetDto;
      if (cmd === "push") {
        throw { code: "operation_conflict", message: "the remote has commits this branch does not" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useSyncStore();

    await store.requestPush();
    const operation = useOperationStore();
    expect(operation.current?.promptLabel).toBe("Push branch 'main' to remote 'origin'");

    await operation.confirm();

    expect(operation.status).toBe("failed");
    expect(operation.error?.message).toBe("the remote has commits this branch does not");
    expect(store.lastPushResult).toBeNull();
  });
});
