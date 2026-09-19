import { setActivePinia, createPinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useOperationStore, type OperationDescriptor } from "./operation";

function sampleOp(overrides: Partial<OperationDescriptor> = {}): OperationDescriptor {
  return {
    kind: "deleteBranch",
    risk: "moderate",
    promptLabel: "branch 'feature/x'",
    run: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("operation store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("a moderate/destructive operation waits at confirming without running", async () => {
    const store = useOperationStore();
    const op = sampleOp({ risk: "moderate" });

    await store.request(op);

    expect(store.status).toBe("confirming");
    expect(op.run).not.toHaveBeenCalled();
  });

  it("cancelling before confirm never runs the operation (US-061 criterion 2)", async () => {
    const store = useOperationStore();
    const op = sampleOp({ risk: "destructive" });
    await store.request(op);

    store.cancel();

    expect(store.status).toBe("idle");
    expect(store.current).toBeNull();
    expect(op.run).not.toHaveBeenCalled();
  });

  it("confirming a pending moderate operation runs it and reaches succeeded", async () => {
    const store = useOperationStore();
    const op = sampleOp({ risk: "moderate" });
    await store.request(op);

    await store.confirm();

    expect(op.run).toHaveBeenCalledTimes(1);
    expect(store.status).toBe("succeeded");
  });

  it("a safe operation skips confirmation and runs immediately", async () => {
    const store = useOperationStore();
    const op = sampleOp({ risk: "safe" });

    await store.request(op);

    expect(op.run).toHaveBeenCalledTimes(1);
    expect(store.status).toBe("succeeded");
  });

  it("a failing operation reaches failed with a non-generic error message", async () => {
    const store = useOperationStore();
    const op = sampleOp({
      risk: "moderate",
      run: vi.fn().mockRejectedValue({ code: "operation_conflict", message: "the branch has unmerged commits" }),
    });
    await store.request(op);

    await store.confirm();

    expect(store.status).toBe("failed");
    expect(store.error?.message).toBe("the branch has unmerged commits");
    expect(store.error?.message).not.toBe("error");
  });

  it("confirm is a no-op without a pending confirmation", async () => {
    const store = useOperationStore();

    await store.confirm();

    expect(store.status).toBe("idle");
  });

  it("dismissing a terminal state clears it back to idle", async () => {
    const store = useOperationStore();
    const op = sampleOp({ risk: "safe" });
    await store.request(op);
    expect(store.status).toBe("succeeded");

    store.cancel();

    expect(store.status).toBe("idle");
    expect(store.current).toBeNull();
  });

  it("a new request replaces whatever was previously pending", async () => {
    const store = useOperationStore();
    await store.request(sampleOp({ risk: "moderate", promptLabel: "first" }));
    expect(store.status).toBe("confirming");

    await store.request(sampleOp({ risk: "moderate", promptLabel: "second" }));

    expect(store.current?.promptLabel).toBe("second");
  });
});
