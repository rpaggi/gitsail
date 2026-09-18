import { describe, expect, it } from "vitest";

import { NO_REMOTES_TEXT, NO_STASH_ENTRIES_TEXT, NO_TAGS_TEXT } from "./referencesPresentation";

// Locks in parity with `gitsail-tui`'s own empty-state copy
// (`crates/gitsail-tui/src/ui.rs::render_references_panel`) — this story's
// DoD requires the same data/semantics as the TUI, including this text.
describe("references empty-state copy matches the TUI verbatim", () => {
  it("tags", () => {
    expect(NO_TAGS_TEXT).toBe("No tags.");
  });

  it("remotes", () => {
    expect(NO_REMOTES_TEXT).toBe("No remotes configured.");
  });

  it("stash", () => {
    expect(NO_STASH_ENTRIES_TEXT).toBe("No stash entries.");
  });
});
