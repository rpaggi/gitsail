import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

import ConfirmationDialog from "./ConfirmationDialog.vue";
import { useOperationStore } from "../stores/operation";

function mountDialog() {
  return mount(ConfirmationDialog, {
    attachTo: document.body,
    global: { plugins: [createPinia()] },
  });
}

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((innerResolve) => {
    resolve = innerResolve;
  });
  return { promise, resolve };
}

describe("ConfirmationDialog Escape + inProgress focus (T-269)", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = "";
  });

  it("Escape in confirming returns to idle without running the operation", async () => {
    const wrapper = mountDialog();
    const store = useOperationStore();
    const run = vi.fn().mockResolvedValue(undefined);
    await store.request({ kind: "deleteBranch", risk: "moderate", promptLabel: "Delete branch 'x'", run });
    await nextTick();

    const dialog = wrapper.find(".confirmation-dialog");
    expect(dialog.exists()).toBe(true);
    await dialog.trigger("keydown", { key: "Escape" });

    expect(store.status).toBe("idle");
    expect(store.current).toBeNull();
    expect(run).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("Escape in succeeded dismisses back to idle", async () => {
    const wrapper = mountDialog();
    const store = useOperationStore();
    await store.request({
      kind: "stageFiles",
      risk: "safe",
      promptLabel: "Stage file 'a.txt'",
      run: vi.fn().mockResolvedValue(undefined),
    });
    await nextTick();
    expect(store.status).toBe("succeeded");

    await wrapper.find(".confirmation-dialog").trigger("keydown", { key: "Escape" });

    expect(store.status).toBe("idle");
    wrapper.unmount();
  });

  it("Escape in failed dismisses back to idle", async () => {
    const wrapper = mountDialog();
    const store = useOperationStore();
    await store.request({
      kind: "deleteBranch",
      risk: "moderate",
      promptLabel: "Delete branch 'x'",
      run: vi.fn().mockRejectedValue({ code: "internal", message: "boom" }),
    });
    await store.confirm();
    await nextTick();
    expect(store.status).toBe("failed");

    await wrapper.find(".confirmation-dialog").trigger("keydown", { key: "Escape" });

    expect(store.status).toBe("idle");
    wrapper.unmount();
  });

  it("Escape during inProgress is a no-op (started work cannot be un-started)", async () => {
    const wrapper = mountDialog();
    const store = useOperationStore();
    const gate = deferred();
    await store.request({
      kind: "deleteBranch",
      risk: "moderate",
      promptLabel: "Delete branch 'x'",
      run: () => gate.promise,
    });
    const pending = store.confirm();
    await nextTick();
    expect(store.status).toBe("inProgress");

    await wrapper.find(".confirmation-dialog").trigger("keydown", { key: "Escape" });

    expect(store.status).toBe("inProgress");
    expect(store.current?.promptLabel).toBe("Delete branch 'x'");
    gate.resolve();
    await pending;
    expect(store.status).toBe("succeeded");
    wrapper.unmount();
  });

  it("keeps focus inside the dialog while inProgress (no focusable button rendered)", async () => {
    const wrapper = mountDialog();
    const store = useOperationStore();
    const gate = deferred();
    await store.request({
      kind: "deleteBranch",
      risk: "moderate",
      promptLabel: "Delete branch 'x'",
      run: () => gate.promise,
    });
    const pending = store.confirm();
    await nextTick();
    await nextTick();
    expect(store.status).toBe("inProgress");

    const dialog = wrapper.find(".confirmation-dialog");
    expect(dialog.exists()).toBe(true);
    expect(dialog.attributes("tabindex")).toBe("-1");
    expect(document.activeElement).toBe(dialog.element);

    // Focus is on the dialog root, so a real Escape keydown still reaches
    // the dialog handler (and stays a no-op while running).
    await dialog.trigger("keydown", { key: "Escape" });
    expect(store.status).toBe("inProgress");

    gate.resolve();
    await pending;
    wrapper.unmount();
  });
});
