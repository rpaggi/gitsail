// The typed frontend service for GitSail's own local UI preferences
// (T-248/US-106), following `repository.ts`'s convention: this is the only
// module that calls `invoke("get_preferences" | "set_theme", ...)` —
// components/stores never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type { PreferencesDto, ThemePreferenceDto } from "./dto";

export async function getPreferences(): Promise<PreferencesDto> {
  return invoke<PreferencesDto>("get_preferences");
}

export async function setTheme(theme: ThemePreferenceDto): Promise<PreferencesDto> {
  return invoke<PreferencesDto>("set_theme", { theme });
}
