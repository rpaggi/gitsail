// The typed frontend service for copying/exporting a patch (US-029/T-162),
// following `repository.ts`'s convention: this is the only module that
// calls `invoke("export_patch" | "save_text_file", ...)` — components/
// stores never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type { ApplyPatchResultDto, PatchExportDto, PatchPreviewDto } from "./dto";

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

/**
 * Reads `path`'s full contents as UTF-8 text — the read counterpart to
 * {@link saveTextFile}, used by T-163/US-030's "apply a patch from a
 * chosen file" flow: the frontend picks `path` via
 * `@tauri-apps/plugin-dialog`'s native open dialog.
 */
export async function readTextFile(path: string): Promise<string> {
  return invoke<string>("read_text_file", { path });
}

/**
 * Validates `patchText` against the repository's current state via a
 * non-mutating `git apply --check` (T-163/US-030 criterion 1). Never
 * mutates anything — the preview a caller shows before asking to confirm
 * `applyPatch`.
 */
export async function previewPatchApplication(patchText: string): Promise<PatchPreviewDto> {
  return invoke<PatchPreviewDto>("preview_patch_application", { patchText });
}

/**
 * Applies `patchText` to the working tree (T-163/US-030) — the confirmed
 * counterpart to {@link previewPatchApplication}. The backend re-validates
 * with the same `--check` immediately before writing anything, so a stale
 * confirmation is refused rather than silently (or partially) applied.
 */
export async function applyPatch(patchText: string): Promise<ApplyPatchResultDto> {
  return invoke<ApplyPatchResultDto>("apply_patch", { patchText });
}
