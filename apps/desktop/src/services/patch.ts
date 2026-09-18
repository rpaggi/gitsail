// The typed frontend service for copying/exporting a patch (US-029/T-162),
// following `repository.ts`'s convention: this is the only module that
// calls `invoke("export_patch" | "save_text_file", ...)` — components/
// stores never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type { PatchExportDto } from "./dto";

/**
 * Builds the full `git apply`-compatible patch for the diff scoped by
 * `staged`/`path` (mirrors `gitsail_application::DiffRequest`'s
 * `staged`/`path_filter`). Read-only: the backend command never touches
 * the index, working tree, or HEAD (US-029 criterion 2).
 */
export async function exportPatch(staged: boolean, path?: string): Promise<PatchExportDto> {
  return invoke<PatchExportDto>("export_patch", { staged, path: path ?? null });
}

/**
 * Writes `contents` to `path`, overwriting whatever was already there —
 * the file-based fallback US-029 criterion 3 requires when the clipboard is
 * unavailable or fails. `path` is always chosen by the person through the
 * native save dialog (see `stores/patchExport.ts`), never invented here.
 */
export async function saveTextFile(path: string, contents: string): Promise<void> {
  return invoke<void>("save_text_file", { path, contents });
}
