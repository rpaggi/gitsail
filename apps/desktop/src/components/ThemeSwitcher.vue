<script setup lang="ts">
// Dark/light theme switcher (T-248/US-106). A two-button toggle group
// rather than a `<select>` or an arbitrary color picker: US-106's DoD
// explicitly excludes a free-color theming system, only dark/light. Each
// button's own visible text ("Dark"/"Light") plus `aria-pressed` on the
// active one is the non-color signal every state needs (US-106 criterion
// 3), so the active choice is never conveyed by color/border alone.

import { usePreferencesStore } from "../stores/preferences";
import type { ThemePreferenceDto } from "../services/dto";

const preferences = usePreferencesStore();

function choose(theme: ThemePreferenceDto): void {
  void preferences.setTheme(theme);
}
</script>

<template>
  <div class="theme-switcher">
    <span class="sr-only" id="gitsail-theme-switcher-label">Theme</span>
    <div class="theme-switcher__buttons" role="group" aria-labelledby="gitsail-theme-switcher-label">
      <button
        type="button"
        :aria-pressed="preferences.effectiveTheme === 'dark'"
        :class="{ 'theme-switcher__button--active': preferences.effectiveTheme === 'dark' }"
        @click="choose('dark')"
      >
        Dark
      </button>
      <button
        type="button"
        :aria-pressed="preferences.effectiveTheme === 'light'"
        :class="{ 'theme-switcher__button--active': preferences.effectiveTheme === 'light' }"
        @click="choose('light')"
      >
        Light
      </button>
    </div>
    <p v-if="preferences.lastError" class="theme-switcher__error" role="alert">
      Could not save theme: {{ preferences.lastError.message }}
    </p>
    <p v-if="preferences.diagnostic" class="theme-switcher__hint" role="status">
      Your saved theme could not be read, so it was reset to the default.
    </p>
  </div>
</template>

<style scoped>
.theme-switcher__buttons {
  display: flex;
  gap: 0.4rem;
  flex-wrap: wrap;
}
.theme-switcher__buttons button {
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0.3rem 0.75rem;
  color: var(--color-text-muted);
  cursor: pointer;
}
.theme-switcher__button--active {
  border-color: var(--color-accent) !important;
  color: var(--color-text) !important;
  font-weight: 600;
}
.theme-switcher__error {
  color: var(--color-danger);
  font-size: 0.8rem;
  margin: 0.35rem 0 0;
}
.theme-switcher__hint {
  color: var(--color-warning);
  font-size: 0.8rem;
  margin: 0.35rem 0 0;
}
</style>
