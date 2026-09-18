<script setup lang="ts">
import { useRepositorySessionStore } from "../stores/session";
import { usePatchExportStore } from "../stores/patchExport";
import type { FileChangeDto, FileStatusCodeDto } from "../services/dto";
import PatchApplyPanel from "./PatchApplyPanel.vue";

const session = useRepositorySessionStore();
const patchExport = usePatchExportStore();

function refresh(): void {
  // The manual refresh trigger (US-054 criterion 2) — the same shared
  // `refreshStatus` action a window-focus event and a future
  // post-mutation refresh also call.
  void session.refreshStatus("manual");
}

// Mirrors `gitsail_tui::status_view::is_relevant` (US-046/US-029): the
// index side has nothing to show for a purely untracked/ignored file, and
// the working-tree side legitimately includes `untracked` (a new, unstaged
// file) but never `unmodified`/`ignored`. A file with both a staged and a
// worktree change offers both buttons, each scoped independently — the
// same "two independently selectable entries" rule the TUI's status list
// applies.
function isRelevant(code: FileStatusCodeDto, scope: "staged" | "worktree"): boolean {
  if (scope === "staged") {
    return code !== "unmodified" && code !== "untracked" && code !== "ignored";
  }
  return code !== "unmodified" && code !== "ignored";
}

/** Copies (or saves, on clipboard failure) the patch for `file`'s `staged`
 * or unstaged side (US-029 criterion 1: the scope label names exactly
 * which side and which file this covers). */
async function copyPatch(file: FileChangeDto, staged: boolean): Promise<void> {
  const scope = `${staged ? "Staged" : "Unstaged"} changes — ${file.path}`;
  await patchExport.exportPatch(staged, file.path, scope);
}

function outcomeMessage(): string {
  const outcome = patchExport.lastOutcome;
  if (outcome === null) return "";
  switch (outcome.kind) {
    case "copied":
      return `Patch copied to clipboard — ${outcome.scope} (${outcome.fileCount} file${
        outcome.fileCount === 1 ? "" : "s"
      })${outcome.incomplete ? " [incomplete: binary/truncated content skipped]" : ""}`;
    case "savedToFile":
      return `Clipboard unavailable — patch for ${outcome.scope} saved to ${outcome.path}${
        outcome.incomplete ? " [incomplete: binary/truncated content skipped]" : ""
      }`;
    case "cancelled":
      return "Clipboard unavailable and no file was chosen — nothing was copied or saved.";
    case "empty":
      return "Nothing to export — no content hunks in this diff.";
    case "failed":
      return `Patch export failed: ${outcome.error.message}`;
  }
}
</script>

<template>
  <div class="status-panel">
    <template v-if="session.repository">
      <p>
        <strong>{{ session.repository.currentBranch ?? "(detached)" }}</strong>
        <template v-if="session.status">
          — {{ session.status.isClean ? "clean" : "dirty" }}
        </template>
        <button :disabled="session.isRefreshing" @click="refresh">
          {{ session.isRefreshing ? "Refreshing…" : "Refresh" }}
        </button>
      </p>
      <p v-if="patchExport.lastOutcome" class="patch-export-outcome">
        {{ outcomeMessage() }}
        <button @click="patchExport.dismiss()">Dismiss</button>
      </p>
      <ul v-if="session.status && !session.status.isClean">
        <li v-for="file in session.status.files" :key="file.path">
          {{ file.worktreeStatus }} {{ file.path }}
          <button
            v-if="isRelevant(file.indexStatus, 'staged')"
            :disabled="patchExport.isExporting"
            :aria-label="`Copy staged patch for ${file.path}`"
            @click="copyPatch(file, true)"
          >
            Copy staged patch
          </button>
          <button
            v-if="isRelevant(file.worktreeStatus, 'worktree')"
            :disabled="patchExport.isExporting"
            :aria-label="`Copy patch for ${file.path}`"
            @click="copyPatch(file, false)"
          >
            Copy patch
          </button>
        </li>
      </ul>
      <PatchApplyPanel />
    </template>
    <p v-else>No repository open.</p>
  </div>
</template>
