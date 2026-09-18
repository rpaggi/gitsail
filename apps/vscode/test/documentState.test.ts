import { describe, expect, it } from "vitest";

import { classifyDocumentSyncState, describeSyncState } from "../src/documentState";

describe("classifyDocumentSyncState (T-204 criterion 3)", () => {
  it("classifies a saved on-disk file", () => {
    const state = classifyDocumentSyncState({
      uri: { scheme: "file", fsPath: "/repo/src/main.rs" },
      isDirty: false,
      isUntitled: false,
    });
    expect(state).toEqual({ kind: "saved", path: "/repo/src/main.rs" });
  });

  it("classifies a dirty on-disk file distinctly from a saved one", () => {
    const state = classifyDocumentSyncState({
      uri: { scheme: "file", fsPath: "/repo/src/main.rs" },
      isDirty: true,
      isUntitled: false,
    });
    expect(state).toEqual({ kind: "unsaved-changes", path: "/repo/src/main.rs" });
  });

  it("classifies an untitled buffer distinctly from a non-file scheme", () => {
    const state = classifyDocumentSyncState({
      uri: { scheme: "untitled", fsPath: "Untitled-1" },
      isDirty: true,
      isUntitled: true,
    });
    expect(state).toEqual({ kind: "untitled" });
  });

  it("classifies a non-file, non-untitled document (e.g. an output channel view)", () => {
    const state = classifyDocumentSyncState({
      uri: { scheme: "output", fsPath: "extension-output" },
      isDirty: false,
      isUntitled: false,
    });
    expect(state).toEqual({ kind: "non-file" });
  });

  it("classifies the absence of an active document", () => {
    expect(classifyDocumentSyncState(undefined)).toEqual({ kind: "non-file" });
  });
});

describe("describeSyncState", () => {
  it("warns only for unsaved changes, mentioning that disk content is what GitSail sees", () => {
    const note = describeSyncState({ kind: "unsaved-changes", path: "/repo/src/main.rs" });
    expect(note).toMatch(/disk/i);
    expect(note).toMatch(/unsaved/i);
  });

  it("has nothing to say for a saved file", () => {
    expect(describeSyncState({ kind: "saved", path: "/repo/src/main.rs" })).toBeUndefined();
  });

  it("has nothing to say for an untitled or non-file document", () => {
    expect(describeSyncState({ kind: "untitled" })).toBeUndefined();
    expect(describeSyncState({ kind: "non-file" })).toBeUndefined();
  });
});
