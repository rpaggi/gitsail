// Copies text to the system clipboard via the browser Clipboard API — the
// same `navigator.clipboard` primitive `CommitGraph.vue`'s hash-copy button
// already uses (this codebase's existing clipboard convention). Kept as its
// own tiny module, rather than inlined in `stores/patchExport.ts`, because
// US-029 criterion 3 needs the failure path distinguished (`false`) from a
// silently swallowed one — the existing hash-copy call uses `?.` and never
// checks whether the write actually succeeded, which is fine for a
// low-stakes convenience copy but not for the primary action of this story.
//
// No Tauri clipboard plugin (`@tauri-apps/plugin-clipboard-manager`) was
// added for this: the browser API already works inside a Tauri v2 webview
// and is the pattern this codebase already established, so introducing a
// second clipboard mechanism would duplicate a capability already present
// for no behavior this story needs.
export async function copyToClipboard(text: string): Promise<boolean> {
  if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
    return false;
  }
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Denied permission, no secure context, or any other webview-specific
    // refusal — all treated the same: the clipboard was not available for
    // this write, so the caller falls back to saving a file.
    return false;
  }
}
