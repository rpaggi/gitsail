<script setup lang="ts">
// Apply a patch pasted or loaded from a chosen file, with a non-mutating
// preview shown before confirmation (T-163/US-030 — the inverse of
// `StatusPanel.vue`'s "Copy patch" buttons, which only ever export). The
// actual confirm/run/result flow is `usePatchApplyStore`'s job; this
// component only collects the patch text and shows the preview it built.

import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";

import { readTextFile } from "../services/patch";
import { usePatchApplyStore } from "../stores/patchApply";

const patchApply = usePatchApplyStore();
const fileError = ref<string | null>(null);

function onTextInput(event: Event): void {
  patchApply.setPatchText((event.target as HTMLTextAreaElement).value);
}

/** Loads a patch file the person picks via the native open dialog into the
 * same text box a pasted patch would fill — never applied without going
 * through the same preview/confirm flow either way. */
async function chooseFile(): Promise<void> {
  fileError.value = null;
  const path = await open({ multiple: false, directory: false });
  if (path === null || Array.isArray(path)) {
    return;
  }
  try {
    const contents = await readTextFile(path);
    patchApply.setPatchText(contents);
  } catch (error) {
    fileError.value = error instanceof Error ? error.message : String(error);
  }
}
</script>

<template>
  <div class="patch-apply-panel">
    <h3>Apply a patch</h3>
    <textarea
      class="patch-apply-textarea"
      placeholder="Paste a patch here, or choose a file below…"
      :value="patchApply.patchText"
      @input="onTextInput"
    ></textarea>
    <p>
      <button @click="chooseFile">Choose file…</button>
      <button :disabled="patchApply.isLoadingPreview || patchApply.patchText.trim().length === 0" @click="patchApply.loadPreview()">
        {{ patchApply.isLoadingPreview ? "Checking…" : "Preview" }}
      </button>
    </p>
    <p v-if="fileError" class="patch-apply-error">Could not read that file: {{ fileError }}</p>
    <p v-if="patchApply.lastError" class="patch-apply-error">
      {{ patchApply.lastError.message }}
      <button @click="patchApply.dismiss()">Dismiss</button>
    </p>
    <template v-if="patchApply.preview">
      <p v-if="patchApply.preview.supported" class="patch-apply-preview patch-apply-preview--supported">
        This patch affects {{ patchApply.preview.affectedFiles.length }}
        file{{ patchApply.preview.affectedFiles.length === 1 ? "" : "s" }}:
      </p>
      <p v-else class="patch-apply-preview patch-apply-preview--rejected">
        Patch rejected: {{ patchApply.preview.rejectionReason }}
      </p>
      <ul v-if="patchApply.preview.affectedFiles.length > 0">
        <li v-for="file in patchApply.preview.affectedFiles" :key="file">{{ file }}</li>
      </ul>
      <button v-if="patchApply.preview.supported" @click="patchApply.requestApply()">Apply patch</button>
    </template>
    <p v-if="patchApply.lastResult" class="patch-apply-result">
      Patch applied — {{ patchApply.lastResult.appliedFiles.length }}
      file{{ patchApply.lastResult.appliedFiles.length === 1 ? "" : "s" }} changed.
      <button @click="patchApply.dismiss()">Dismiss</button>
    </p>
  </div>
</template>
