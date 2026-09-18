// Shared modal-dialog focus wiring (US-055 criterion 1) for
// `ConfirmationDialog.vue` and `HistoryEditingPanel.vue` — the two overlay
// dialogs mounted globally in `App.vue`. This module is deliberately *not*
// "pure logic" (it touches the DOM: `querySelectorAll`, `document
// .activeElement`, `.focus()`) and is not unit tested on its own — the
// boundary-detection math it relies on, `tabTrapIndex` (`keyboardNav.ts`),
// is what's pure and is what has the tests. This is only the thin,
// otherwise-duplicated DOM wiring around that math, kept in one place
// rather than copy-pasted into both dialog components.

import { tabTrapIndex } from "./keyboardNav";

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function focusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
}

/** Moves focus to the first focusable element inside `container` — call
 * once a dialog becomes visible (e.g. from a `watch` on the store field
 * that controls it), so keyboard/screen-reader users land somewhere
 * actionable immediately instead of on whatever had focus on the page
 * behind the now-open overlay. */
export function focusFirst(container: HTMLElement | null | undefined): void {
  const first = container ? focusableElements(container)[0] : undefined;
  first?.focus();
}

/**
 * Keeps Tab/Shift+Tab inside `container` (WCAG 2.1.2 "No Keyboard Trap" —
 * a modal dialog must trap focus *deliberately*, so Tab never silently
 * escapes to the page underneath it). Bind as the dialog root's `keydown`
 * handler. A no-op for any key but Tab, and for Tab anywhere except the
 * first/last focusable element, where the browser's own default focus
 * order already does the right thing — this only intervenes at the two
 * boundaries.
 */
export function handleFocusTrapKeydown(container: HTMLElement, event: KeyboardEvent): void {
  if (event.key !== "Tab") {
    return;
  }
  const elements = focusableElements(container);
  if (elements.length === 0) {
    return;
  }
  const currentIndex = elements.indexOf(document.activeElement as HTMLElement);
  const next = tabTrapIndex(currentIndex < 0 ? 0 : currentIndex, event, elements.length);
  if (next === null) {
    return;
  }
  event.preventDefault();
  elements[next]?.focus();
}
