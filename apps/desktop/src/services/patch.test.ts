import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { exportPatch, saveTextFile } from "./patch";
import type { PatchExportDto } from "./dto";

describe("patch service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("exportPatch invokes export_patch with staged and an omitted path as null", async () => {
    const dto: PatchExportDto = {
      patch: "--- a/a.txt\n+++ b/a.txt\n",
      includedFiles: ["a.txt"],
      skippedBinaryFiles: [],
      skippedTruncatedFiles: [],
    };
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return dto;
    });

    const result = await exportPatch(true);

    expect(receivedCommand).toBe("export_patch");
    expect(receivedArgs).toEqual({ staged: true, path: null });
    expect(result).toEqual(dto);
  });

  it("exportPatch forwards an explicit path", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return { patch: "", includedFiles: [], skippedBinaryFiles: [], skippedTruncatedFiles: [] };
    });

    await exportPatch(false, "src/lib.rs");

    expect(receivedArgs).toEqual({ staged: false, path: "src/lib.rs" });
  });

  it("saveTextFile invokes save_text_file with the given path and contents", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await saveTextFile("/tmp/out.patch", "the patch text");

    expect(receivedCommand).toBe("save_text_file");
    expect(receivedArgs).toEqual({ path: "/tmp/out.patch", contents: "the patch text" });
  });
});
