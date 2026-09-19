import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useBranchesStore } from "./branches";
import type { BranchDto } from "../services/dto";

function branch(name: string, isCurrent: boolean): BranchDto {
  return {
    name,
    kind: { kind: "local" },
    target: "a".repeat(40),
    upstream: null,
    ahead: 0,
    behind: 0,
    isCurrent,
  };
}

describe("branches store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("load populates the branch list", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_branches") return [branch("main", true), branch("feature/x", false)];
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useBranchesStore();

    await store.load();

    expect(store.branches).toHaveLength(2);
    expect(store.lastError).toBeNull();
  });

  it("load records a failure without throwing", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "boom" };
    });
    const store = useBranchesStore();

    await store.load();

    expect(store.branches).toEqual([]);
    expect(store.lastError?.message).toBe("boom");
  });

  it("requestCreate is Moderate risk; confirming creates the branch and reloads the list", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "list_branches") return [branch("main", true), branch("feature/y", false)];
      return null;
    });
    const store = useBranchesStore();

    await store.requestCreate("feature/y");
    expect(received).toEqual([]);

    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    await operation.confirm();

    expect(received).toContain("create_branch");
    expect(received).toContain("list_branches");
    expect(store.branches.some((b) => b.name === "feature/y")).toBe(true);
  });

  it("requestSwitch (Moderate) waits for explicit confirmation before switching", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "list_branches") return [branch("develop", true)];
      return null;
    });
    const store = useBranchesStore();

    await store.requestSwitch("develop");

    expect(received).toEqual([]);
    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");

    await operation.confirm();

    expect(received).toContain("switch_branch");
    expect(received).toContain("list_branches");
    expect(operation.status).toBe("succeeded");
  });

  it("requestDelete with force is Destructive and carries a non-generic impact message", async () => {
    const store = useBranchesStore();

    await store.requestDelete("feature/x", true);

    // Import lazily to avoid a module-order issue with Pinia's active
    // instance in this test file.
    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("destructive");
    expect(operation.current?.impact).toContain("feature/x");
    expect(operation.current?.impact).not.toBe("");
  });

  it("requestDelete without force is Moderate risk with no extra impact text", async () => {
    const store = useBranchesStore();

    await store.requestDelete("feature/x", false);

    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.impact).toBeUndefined();
  });

  it("requestRename (Moderate) waits for explicit confirmation, then renames and reloads the list", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "list_branches") return [branch("feature/renamed", false)];
      return null;
    });
    const store = useBranchesStore();

    await store.requestRename("feature/old", "feature/renamed");

    expect(received).toEqual([]);
    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.promptLabel).toContain("feature/old");
    expect(operation.current?.promptLabel).toContain("feature/renamed");

    await operation.confirm();

    expect(received).toContain("rename_branch");
    expect(received).toContain("list_branches");
    expect(operation.status).toBe("succeeded");
    expect(store.branches.some((b) => b.name === "feature/renamed")).toBe(true);
  });

  // T-267: US-061 criterion 1 ("never a generic 'are you sure?'") is not
  // satisfied by naming the target alone. Creating, checking out and
  // deleting the same branch used to open the identical dialog — "branch
  // 'feature/x'" — and the risk badge does not separate them either, since
  // `moderate` covers both the checkout and the plain delete.
  it("three different operations on one branch ask three different questions", async () => {
    mockIPC(() => null);
    const store = useBranchesStore();
    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();

    const prompts: string[] = [];
    for (const request of [
      () => store.requestCreate("feature/x"),
      () => store.requestSwitch("feature/x"),
      () => store.requestDelete("feature/x", false),
      () => store.requestDelete("feature/x", true),
    ]) {
      await request();
      const prompt = operation.current?.promptLabel ?? "";
      expect(prompt).toContain("feature/x");
      prompts.push(prompt);
      operation.cancel();
    }

    expect(new Set(prompts).size).toBe(prompts.length);
    expect(prompts[1]).toBe("Check out branch 'feature/x'");
    expect(prompts[2]).toBe("Delete branch 'feature/x'");
  });

  it("requestRename never overwrites a colliding branch: a refused rename surfaces as a failed operation", async () => {
    mockIPC((cmd) => {
      if (cmd === "rename_branch") {
        throw { code: "invalid_repository_state", message: "a branch with that name already exists" };
      }
      if (cmd === "list_branches") return [branch("main", true), branch("feature/x", false)];
      return null;
    });
    const store = useBranchesStore();

    await store.requestRename("feature/y", "feature/x");
    const { useOperationStore } = await import("./operation");
    const operation = useOperationStore();
    await operation.confirm();

    expect(operation.status).toBe("failed");
    expect(operation.error?.message).toContain("already exists");
  });
});
