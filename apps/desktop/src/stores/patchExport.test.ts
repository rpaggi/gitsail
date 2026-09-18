import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { usePatchExportStore } from "./patchExport";
import type { PatchExportDto } from "../services/dto";

function patchExportDto(overrides: Partial<PatchExportDto> = {}): PatchExportDto {
  return {
    patch: "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n",
    includedFiles: ["a.txt"],
    skippedBinaryFiles: [],
    skippedTruncatedFiles: [],
    ...overrides,
  };
}

/** Simulates a working `navigator.clipboard.writeText`, recording every
 * call, or an unavailable/failing clipboard when `mode` says so. jsdom does
 * not implement the Clipboard API itself, so every test controls it
 * explicitly rather than depending on an environment default. */
function stubClipboard(mode: "works" | "unavailable" | "throws"): { calls: string[] } {
  const calls: string[] = [];
  if (mode === "unavailable") {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    return { calls };
  }
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: vi.fn(async (text: string) => {
        if (mode === "throws") {
          throw new Error("clipboard write denied");
        }
        calls.push(text);
      }),
    },
  });
  return { calls };
}

describe("patch export store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  });

  it("starts idle with no outcome", () => {
    const store = usePatchExportStore();
    expect(store.isExporting).toBe(false);
    expect(store.lastOutcome).toBeNull();
  });

  it("copies the patch to the clipboard and reports the scope, file count, and completeness", async () => {
    const clipboard = stubClipboard("works");
    const dto = patchExportDto();
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "export_patch") {
        receivedArgs = args;
        return dto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = usePatchExportStore();
    await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");

    expect(receivedArgs).toEqual({ staged: false, path: "a.txt" });
    expect(clipboard.calls).toEqual([dto.patch]);
    expect(store.lastOutcome).toEqual({
      kind: "copied",
      scope: "Unstaged changes — a.txt",
      fileCount: 1,
      incomplete: false,
    });
    expect(store.isExporting).toBe(false);
  });

  it("flags an incomplete export when the backend skipped binary/truncated files", async () => {
    stubClipboard("works");
    mockIPC((cmd) => {
      if (cmd === "export_patch") {
        return patchExportDto({ skippedBinaryFiles: ["image.png"] });
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = usePatchExportStore();
    await store.exportPatch(true, null, "Staged changes");

    expect(store.lastOutcome).toMatchObject({ kind: "copied", incomplete: true });
  });

  it(
    "falls back to the save dialog and writes the file when the clipboard is unavailable " +
      "(US-029 criterion 3)",
    async () => {
      stubClipboard("unavailable");
      const dto = patchExportDto();
      let savedPath = "";
      let savedContents = "";
      mockIPC((cmd, args) => {
        if (cmd === "export_patch") return dto;
        if (cmd === "plugin:dialog|save") return "/chosen/a.txt.patch";
        if (cmd === "save_text_file") {
          const typed = args as { path: string; contents: string };
          savedPath = typed.path;
          savedContents = typed.contents;
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const store = usePatchExportStore();
      await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");

      expect(savedPath).toBe("/chosen/a.txt.patch");
      expect(savedContents).toBe(dto.patch);
      expect(store.lastOutcome).toEqual({
        kind: "savedToFile",
        scope: "Unstaged changes — a.txt",
        path: "/chosen/a.txt.patch",
        incomplete: false,
      });
    },
  );

  it("reports cancelled when the clipboard fails and no save location is chosen", async () => {
    stubClipboard("throws");
    mockIPC((cmd) => {
      if (cmd === "export_patch") return patchExportDto();
      if (cmd === "plugin:dialog|save") return null; // the person closed the dialog
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = usePatchExportStore();
    await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");

    expect(store.lastOutcome).toEqual({ kind: "cancelled" });
  });

  it("reports empty without ever touching the clipboard or the save dialog", async () => {
    stubClipboard("works");
    let dialogCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "export_patch") {
        return patchExportDto({ patch: "", includedFiles: [] });
      }
      if (cmd === "plugin:dialog|save") {
        dialogCalls += 1;
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = usePatchExportStore();
    await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");

    expect(store.lastOutcome).toEqual({ kind: "empty" });
    expect(dialogCalls).toBe(0);
  });

  it("reports failed with the backend's error payload when export_patch itself fails", async () => {
    stubClipboard("works");
    mockIPC(() => {
      throw { code: "invalid_repository_state", message: "no repository is open" };
    });

    const store = usePatchExportStore();
    await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");

    expect(store.lastOutcome).toEqual({
      kind: "failed",
      error: { code: "invalid_repository_state", message: "no repository is open" },
    });
  });

  it("dismiss clears the last outcome", async () => {
    stubClipboard("works");
    mockIPC((cmd) => {
      if (cmd === "export_patch") return patchExportDto();
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePatchExportStore();
    await store.exportPatch(false, "a.txt", "Unstaged changes — a.txt");
    expect(store.lastOutcome).not.toBeNull();

    store.dismiss();

    expect(store.lastOutcome).toBeNull();
  });
});
