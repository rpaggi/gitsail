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
});
