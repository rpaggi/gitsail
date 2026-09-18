// Pure theme-resolution logic (T-248/US-106), kept separate from
// `stores/preferences.ts` (Tauri/Pinia wiring) so "what do we actually
// render" is directly unit-testable — mirroring `keybindings.ts`'s own
// pure/impure split.
//
// **Dark is the initial theme** (US-106 criterion 1): a person who has
// never made an explicit choice sees dark, matching this app's default
// visual identity (`theme.css`'s top-level `:root` tokens already are the
// dark palette — see that file's own doc comment). This app deliberately
// never reads `prefers-color-scheme`: only two themes are in scope (no
// arbitrary/system theming — DoD), and light must always be a person's own
// explicit choice, never inferred from the OS. `ThemePreference::System`
// (`gitsail_application`'s own default, before any explicit choice is ever
// persisted) therefore resolves to the dark palette here, exactly like an
// explicit `"dark"` choice would.

import type { ThemePreferenceDto } from "./services/dto";

export type EffectiveTheme = "dark" | "light";

export function resolveEffectiveTheme(preference: ThemePreferenceDto): EffectiveTheme {
  return preference === "light" ? "light" : "dark";
}

/**
 * Applies `theme` to the document root via a `data-theme` attribute — the
 * seam `theme.css`'s `:root[data-theme="light"]` override block hooks
 * into. Dark needs no attribute at all to render correctly (it is
 * `:root`'s own default palette, so a person sees it before this function
 * ever runs), but this still sets `data-theme="dark"` explicitly so
 * switching back to light later always has a stable, always-present
 * attribute to toggle rather than special-casing "no attribute yet" vs.
 * "explicitly dark".
 */
export function applyEffectiveTheme(
  theme: EffectiveTheme,
  root: HTMLElement = document.documentElement,
): void {
  root.setAttribute("data-theme", theme);
}
