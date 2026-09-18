// GitSail's own local UI preferences (T-248/US-106), following
// `appData.ts`'s convention: a thin Pinia wrapper over a typed service
// (`../services/preferences.ts`), never calling `invoke` directly.
//
// This replaces the empty placeholder this store shipped as in T-184/
// US-051 ("US-053 blocked on US-129") — both blockers (US-053/T-186 and
// US-105/T-247) are resolved now, so this is the first story to actually
// fill it in.

import { defineStore } from "pinia";

import { getPreferences, setTheme as setThemeCommand } from "../services/preferences";
import type { ThemePreferenceDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { applyEffectiveTheme, resolveEffectiveTheme, type EffectiveTheme } from "../theme";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const usePreferencesStore = defineStore("preferences", {
  state: () => ({
    // Dark-looking default even before `load()` resolves (US-106 criterion
    // 1) — matches `theme.css`'s own unconditional `:root` palette, so
    // there is no light-then-dark flash while this store's very first
    // `getPreferences()` call is still in flight.
    theme: "dark" as ThemePreferenceDto,
    isLoading: false,
    lastError: null as ErrorPayload | null,
    /** Set when the persisted preferences file was corrupted/unreadable
     * and silently recovered to defaults (`gitsail_application::
     * PreferencesLoadOutcome`'s own contract) — surfaced so the frontend
     * can tell the person their saved theme was reset, rather than them
     * wondering why. */
    diagnostic: null as ErrorPayload | null,
  }),
  getters: {
    effectiveTheme(state): EffectiveTheme {
      return resolveEffectiveTheme(state.theme);
    },
  },
  actions: {
    /** Loads the persisted theme and applies it to the document (US-106
     * criterion 2: persists across sessions). Called once, on app mount
     * (`App.vue`). */
    async load(): Promise<void> {
      this.isLoading = true;
      try {
        const preferences = await getPreferences();
        this.theme = preferences.theme;
        this.diagnostic = preferences.diagnostic ?? null;
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      } finally {
        this.isLoading = false;
        applyEffectiveTheme(this.effectiveTheme);
      }
    },

    /** Switches and persists the theme (US-106 criteria 1/2). Applies
     * immediately, optimistically: a persistence failure is reported via
     * `lastError` but never reverts the theme the person already sees and
     * chose — losing the *next* session's persistence is a lesser failure
     * than silently reverting what is on screen right now. */
    async setTheme(theme: ThemePreferenceDto): Promise<void> {
      this.theme = theme;
      applyEffectiveTheme(this.effectiveTheme);
      try {
        await setThemeCommand(theme);
        this.lastError = null;
      } catch (error) {
        this.lastError = toErrorPayload(error);
      }
    },
  },
});
