// Pure tag/remote/stash presentation constants (T-195/US-062 criterion 2).
//
// Empty-state copy matches `gitsail-tui`'s own `ReferenceView` rendering
// (`crates/gitsail-tui/src/ui.rs::render_references_panel`) verbatim — this
// story's DoD requires query parity with the TUI, down to what "empty"
// says.

export const NO_TAGS_TEXT = "No tags.";
export const NO_REMOTES_TEXT = "No remotes configured.";
export const NO_STASH_ENTRIES_TEXT = "No stash entries.";
