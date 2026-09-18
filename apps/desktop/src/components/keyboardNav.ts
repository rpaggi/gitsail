// Pure keyboard-navigation helpers (T-188/US-055 criterion 1: "navegação de
// teclado cobrem as ações principais"). Two call sites reuse this same
// index arithmetic rather than each hand-rolling it:
//
//  - `AppShell.vue`'s tabs (`role="tablist"`), a small fixed-size,
//    horizontal, wrapping list — arrow keys should cycle at the ends.
//  - `CommitGraph.vue`'s commit list (`role="listbox"`), a large, virtualized,
//    vertical, non-wrapping list — Down/Up move by one row and stop at
//    either end rather than jumping from the last loaded row back to the
//    first (surprising for a list that keeps growing via `loadMore`).
//
// Kept DOM-free and pure so it is unit-testable without `@vue/test-utils`
// (not a project dependency — see this project's test convention: pure
// logic extracted from components is unit tested directly).

export type Orientation = "horizontal" | "vertical";

/**
 * Steps `current` by `direction` (+1/-1) within `[0, count)`. At either
 * boundary: wraps around when `wrap` is true, otherwise returns `null` (no
 * movement — the caller does nothing, rather than clamping to the same
 * index, so "no key was consumed" and "moved to the same index" are never
 * confused).
 */
export function stepIndex(
  current: number,
  direction: 1 | -1,
  count: number,
  wrap: boolean,
): number | null {
  if (count <= 0) {
    return null;
  }
  const next = current + direction;
  if (next >= 0 && next < count) {
    return next;
  }
  return wrap ? (direction > 0 ? 0 : count - 1) : null;
}

/**
 * Resolves the next roving-tabindex/aria-activedescendant index for a
 * single keyboard event, or `null` when the key does not navigate this list
 * (the caller should let the event fall through unhandled). `orientation`
 * picks which arrow keys are "forward"/"backward" (WAI-ARIA APG: a
 * horizontal tablist responds to Left/Right, a vertical listbox to Up/Down).
 * `Home`/`End` always jump to the first/last index regardless of
 * orientation, matching both patterns.
 */
export function rovingNextIndex(
  current: number,
  key: string,
  count: number,
  orientation: Orientation,
  wrap = true,
): number | null {
  const forwardKey = orientation === "horizontal" ? "ArrowRight" : "ArrowDown";
  const backwardKey = orientation === "horizontal" ? "ArrowLeft" : "ArrowUp";
  if (key === forwardKey) {
    return stepIndex(current, 1, count, wrap);
  }
  if (key === backwardKey) {
    return stepIndex(current, -1, count, wrap);
  }
  if (key === "Home") {
    return count > 0 ? 0 : null;
  }
  if (key === "End") {
    return count > 0 ? count - 1 : null;
  }
  return null;
}

/**
 * The boundary-only half of a modal focus trap (WCAG 2.1.2 "No Keyboard
 * Trap" — a dialog must still cycle focus *within* itself, never let Tab
 * escape to the page underneath while it is open): given which index in a
 * dialog's focusable-element list currently has focus, returns the index
 * Tab/Shift+Tab should move to when — and only when — that index is already
 * the first or last one. Every non-boundary Tab press is left to the
 * browser's own default focus order (returns `null`), so this never
 * fights native tab order in the middle of the dialog.
 */
export function tabTrapIndex(
  current: number,
  event: { key: string; shiftKey: boolean },
  count: number,
): number | null {
  if (event.key !== "Tab" || count <= 0) {
    return null;
  }
  const isFirst = current <= 0;
  const isLast = current >= count - 1;
  if (event.shiftKey && isFirst) {
    return count - 1;
  }
  if (!event.shiftKey && isLast) {
    return 0;
  }
  return null;
}
