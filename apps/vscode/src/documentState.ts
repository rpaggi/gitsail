// Unsaved-buffer awareness (T-204 criterion 3).
//
// `gitsail-cli` only ever reads what is actually on disk (SAD §33: all
// repository content GitSail touches goes through Git/the filesystem, not
// an editor buffer) — it has no way to see an unsaved in-memory edit. This
// module classifies the active document's relationship to that disk
// content so a query result can never be silently presented as if it
// described the buffer the user is currently looking at, when it actually
// describes the last-saved version.
//
// This module only classifies the state; attaching that classification to
// a specific piece of UI (a decoration disclaimer on a blame line, for
// example) is EPIC-15's job once there is blame/history content to attach
// it to. Foundation's job is only that the fact is never lost silently.

export interface DocumentLike {
  uri: { scheme: string; fsPath: string };
  isDirty: boolean;
  isUntitled: boolean;
}

export type DocumentSyncState =
  | { kind: "saved"; path: string }
  | { kind: "unsaved-changes"; path: string }
  | { kind: "untitled" }
  | { kind: "non-file" };

export function classifyDocumentSyncState(document: DocumentLike | undefined): DocumentSyncState {
  if (!document) {
    return { kind: "non-file" };
  }
  if (document.uri.scheme !== "file") {
    return document.isUntitled ? { kind: "untitled" } : { kind: "non-file" };
  }
  return document.isDirty
    ? { kind: "unsaved-changes", path: document.uri.fsPath }
    : { kind: "saved", path: document.uri.fsPath };
}

/** A user-facing note for states where GitSail's CLI-backed data could be
 * mistaken for describing the current buffer. `undefined` means no note is
 * needed (there is nothing risky to disclose). */
export function describeSyncState(state: DocumentSyncState): string | undefined {
  if (state.kind === "unsaved-changes") {
    return "GitSail reflects the file as saved on disk; this file has unsaved changes that are not yet included.";
  }
  return undefined;
}
