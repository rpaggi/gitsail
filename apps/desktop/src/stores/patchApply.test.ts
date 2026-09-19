import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { usePatchApplyStore } from "./patchApply";
import { useOperationStore } from "./operation";
import type { PatchPreviewDto } from "../services/dto";

function supportedPreview(files: string[] = ["a.txt"]): PatchPreviewDto {
  return { affectedFiles: files, supported: true, rejectionReason: null };
}

function rejectedPreview(reason: string): PatchPreviewDto {
  return { affectedFiles: [], supported: false, rejectionReason: reason };
}

describe("patchApply store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loadPreview is a no-op for empty patch text", async () => {
    const store = usePatchApplyStore();
    store.setPatchText("   ");

    await store.loadPreview();

    expect(store.preview).toBeNull();
  });

  it("loadPreview stores a supported preview with its affected files", async () => {
    mockIPC((cmd) => {
      if (cmd === "preview_patch_application") return supportedPreview(["a.txt", "b.txt"]);
      return null;
    });
    const store = usePatchApplyStore();
    store.setPatchText("--- a/a.txt\n+++ b/a.txt\n");

    await store.loadPreview();

    expect(store.preview?.supported).toBe(true);
    expect(store.preview?.affectedFiles).toEqual(["a.txt", "b.txt"]);
    expect(store.lastError).toBeNull();
  });

  it("loadPreview stores a rejected preview without treating it as a hard error", async () => {
    mockIPC(() => rejectedPreview("the patch references a path outside the repository: ../../etc/passwd"));
    const store = usePatchApplyStore();
    store.setPatchText("a malicious patch");

    await store.loadPreview();

    expect(store.preview?.supported).toBe(false);
    expect(store.preview?.rejectionReason).toContain("outside the repository");
    expect(store.lastError).toBeNull();
  });

  it("setPatchText clears a previous preview and result so they never refer to stale text", async () => {
    mockIPC(() => supportedPreview());
    const store = usePatchApplyStore();
    store.setPatchText("first patch");
    await store.loadPreview();
    expect(store.preview).not.toBeNull();

    store.setPatchText("a different patch");

    expect(store.preview).toBeNull();
  });

  it("requestApply without a supported preview is a no-op", async () => {
    const store = usePatchApplyStore();
    store.setPatchText("some text");

    await store.requestApply();

    const operation = useOperationStore();
    expect(operation.status).toBe("idle");
  });

  it("requestApply with a rejected preview is a no-op — rejection never reaches confirmation", async () => {
    mockIPC(() => rejectedPreview("the patch is malformed"));
    const store = usePatchApplyStore();
    store.setPatchText("not a patch");
    await store.loadPreview();

    await store.requestApply();

    const operation = useOperationStore();
    expect(operation.status).toBe("idle");
  });

  it("requestApply with a supported preview confirms as Moderate risk naming the file count", async () => {
    mockIPC(() => supportedPreview(["a.txt", "b.txt"]));
    const store = usePatchApplyStore();
    store.setPatchText("--- a/a.txt\n+++ b/a.txt\n");
    await store.loadPreview();

    await store.requestApply();

    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("moderate");
    expect(operation.current?.promptLabel).toContain("2 files");
  });

  it("confirming sends back exactly the previewed patch text and clears it on success", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "preview_patch_application") return supportedPreview(["a.txt"]);
      if (cmd === "apply_patch") {
        receivedArgs = args;
        return { appliedFiles: ["a.txt"] };
      }
      if (cmd === "get_repository_status") {
        return { branch: "main", headState: { state: "attached", branch: "main" }, files: [], isClean: true };
      }
      return null;
    });
    const store = usePatchApplyStore();
    store.setPatchText("--- a/a.txt\n+++ b/a.txt\n");
    await store.loadPreview();

    await store.requestApply();
    const operation = useOperationStore();
    await operation.confirm();

    expect(receivedArgs).toEqual({ patchText: "--- a/a.txt\n+++ b/a.txt\n" });
    expect(store.lastResult?.appliedFiles).toEqual(["a.txt"]);
    expect(store.patchText).toBe("");
    expect(store.preview).toBeNull();
    expect(operation.status).toBe("succeeded");
  });

  it("a stale context between preview and confirmation reports a conflict, never a false success", async () => {
    mockIPC((cmd) => {
      if (cmd === "preview_patch_application") return supportedPreview(["a.txt"]);
      if (cmd === "apply_patch") {
        throw {
          code: "operation_conflict",
          message: "the patch no longer applies to the current file content",
        };
      }
      return null;
    });
    const store = usePatchApplyStore();
    store.setPatchText("--- a/a.txt\n+++ b/a.txt\n");
    await store.loadPreview();

    await store.requestApply();
    const operation = useOperationStore();
    await operation.confirm();

    expect(operation.status).toBe("failed");
    expect(operation.error?.message).toBe("the patch no longer applies to the current file content");
  });
});
