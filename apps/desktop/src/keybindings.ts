// Central registry of Desktop's configurable keyboard shortcuts (T-249/
// US-107). Pure, DOM-free logic only — no `window.addEventListener` call
// lives here — so binding normalization, matching, and conflict detection
// are all unit-testable directly, matching this project's own convention
// of extracting pure logic out of components for direct unit tests (see
// `components/keyboardNav.ts`).
//
// **Binding format**: a canonical string built as `Mod[+Shift][+Alt]+KEY`,
// e.g. `"Mod+K"` or `"Mod+Shift+F"`. `Mod` deliberately abstracts over
// Ctrl (Windows/Linux) vs Cmd (macOS) — `matchesBinding` treats
// `ctrlKey || metaKey` as satisfying `Mod`, the convention most
// cross-platform editors use, so one default binding list serves every
// Desktop platform without per-OS branching. `formatBindingForDisplay`
// renders `Mod` back to a platform-appropriate label ("Ctrl" vs "Cmd") for
// the UI only; the stored/compared string is always the `Mod` form.
//
// **Scope decision**: every capturable binding must include `Mod` —
// `bindingFromKeyboardEvent` returns `null` when it is not held (or when
// the key itself is a bare modifier). A remap UI built on this can never
// record a plain letter/Shift/Alt-only combination, so remapping can never
// silently break ordinary typing in the commit-message textarea, the
// search box, etc. This is this story's own stated scope cut (US-107 says
// nothing about non-modified bindings), not an oversight.

export interface ConfigurableAction {
  id: string;
  label: string;
  defaultBinding: string;
}

/** The actions US-107 makes configurable. Each is wired to a real,
 * already-existing store action (see `AppShell.vue`'s global dispatcher) —
 * this registry is never a list of aspirational/unimplemented shortcuts. */
export const CONFIGURABLE_ACTIONS: ConfigurableAction[] = [
  { id: "focus-search", label: "Focus search", defaultBinding: "Mod+K" },
  { id: "fetch", label: "Fetch", defaultBinding: "Mod+Shift+F" },
  { id: "pull", label: "Pull", defaultBinding: "Mod+Shift+L" },
  { id: "push", label: "Push", defaultBinding: "Mod+Shift+P" },
  { id: "commit", label: "Create commit", defaultBinding: "Mod+Enter" },
];

/** A structural subset of `KeyboardEvent` — pure functions here take this
 * instead of the real DOM type so tests can pass a plain object. */
export interface KeyEventLike {
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  key: string;
}

/** Keys that are themselves modifiers — pressed alone, never a binding. */
const MODIFIER_KEYS = new Set(["Control", "Meta", "Alt", "Shift"]);

function normalizeKeyName(key: string): string {
  // A single printable character (letters, digits, punctuation) is
  // normalized to upper case so "k" and "K" (Shift held or not) produce the
  // same binding; multi-character `KeyboardEvent.key` names (`Enter`,
  // `Escape`, `ArrowUp`, `F1`, ...) are already a stable, human-readable
  // spelling and are kept as-is.
  return key.length === 1 ? key.toUpperCase() : key;
}

/**
 * Builds `event`'s canonical binding string, or `null` when it cannot be
 * one — see this module's own "scope decision" doc above.
 */
export function bindingFromKeyboardEvent(event: KeyEventLike): string | null {
  if (MODIFIER_KEYS.has(event.key)) {
    return null;
  }
  if (!event.ctrlKey && !event.metaKey) {
    return null;
  }
  const parts = ["Mod"];
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  parts.push(normalizeKeyName(event.key));
  return parts.join("+");
}

/** Whether `event` triggers `binding` — the dispatch-time counterpart to
 * [[bindingFromKeyboardEvent]], built independently rather than by
 * comparing its own output to itself, so a regression in either direction
 * is still caught by a test exercising both. */
export function matchesBinding(event: KeyEventLike, binding: string): boolean {
  return bindingFromKeyboardEvent(event) === binding;
}

/** Merges each action's default binding with any persisted override —
 * the one place "what does this action currently do" is resolved, used by
 * both the settings panel and the global dispatcher so they can never
 * disagree with each other. */
export function effectiveBindings(
  actions: ConfigurableAction[],
  overrides: Record<string, string>,
): Record<string, string> {
  const result: Record<string, string> = {};
  for (const action of actions) {
    result[action.id] = overrides[action.id] ?? action.defaultBinding;
  }
  return result;
}

/**
 * Groups action ids by their effective binding, keeping only bindings
 * shared by two or more actions (US-107 criterion 2). A conflict is
 * reported, never silently prevented: whichever remap created it still
 * takes effect (the frontend only warns), since refusing it outright would
 * require picking one of two equally-valid remaps to reject on the
 * person's behalf.
 */
export function findBindingConflicts(bindings: Record<string, string>): Map<string, string[]> {
  const byBinding = new Map<string, string[]>();
  for (const [actionId, binding] of Object.entries(bindings)) {
    const ids = byBinding.get(binding);
    if (ids) {
      ids.push(actionId);
    } else {
      byBinding.set(binding, [actionId]);
    }
  }
  for (const [binding, ids] of byBinding) {
    if (ids.length < 2) {
      byBinding.delete(binding);
    }
  }
  return byBinding;
}

/** `true` when the current platform is macOS (Cmd instead of Ctrl) —
 * display-only; comparisons always go through `Mod` regardless. */
export function isMacPlatform(
  nav: Pick<Navigator, "platform" | "userAgent"> | undefined = typeof navigator === "undefined"
    ? undefined
    : navigator,
): boolean {
  const signal = `${nav?.platform ?? ""} ${nav?.userAgent ?? ""}`;
  return /Mac|iPhone|iPad/.test(signal);
}

/** Renders a canonical binding for display, substituting `Mod` for the
 * platform's own modifier label. Takes `mac` explicitly (defaulting to
 * [[isMacPlatform]]'s live read) so it stays unit-testable for both
 * platforms without mocking `navigator`. */
export function formatBindingForDisplay(binding: string, mac: boolean = isMacPlatform()): string {
  return binding
    .split("+")
    .map((part) => (part === "Mod" ? (mac ? "Cmd" : "Ctrl") : part))
    .join("+");
}
