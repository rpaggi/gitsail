// The typed frontend service for GitSail's own local UI preferences
// (T-248/US-106) and desktop update checking (T-260/US-127), following
// `repository.ts`'s convention: this is the only module that calls
// `invoke("get_preferences" | "set_theme" | "check_for_update" | ..., ...)`
// — components/stores never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";

import type {
  PreferencesDto,
  ThemePreferenceDto,
  UpdateCheckOutcomeDto,
  UpdateCheckTriggerDto,
} from "./dto";

export async function getPreferences(): Promise<PreferencesDto> {
  return invoke<PreferencesDto>("get_preferences");
}

export async function setTheme(theme: ThemePreferenceDto): Promise<PreferencesDto> {
  return invoke<PreferencesDto>("set_theme", { theme });
}

/**
 * Checks GitHub for a newer release than the one this build was published
 * as (T-260/US-127). `trigger` is `"automatic"` (the app-mount call —
 * throttled/disableable, see `gitsail_application::update_check`'s own doc
 * comment) or `"manual"` (an explicit "check for updates now" click, which
 * bypasses both). Never rejects for a network/malformed-response failure —
 * that is one more `UpdateCheckOutcomeDto` variant (`"checkFailed"`), not a
 * thrown error.
 */
export async function checkForUpdate(
  trigger: UpdateCheckTriggerDto,
): Promise<UpdateCheckOutcomeDto> {
  return invoke<UpdateCheckOutcomeDto>("check_for_update", { trigger });
}

/** Toggles the "automatic update check" preference (US-127's mandatory
 * "always possible to disable" control). Never gates a manual check. */
export async function setCheckForUpdates(enabled: boolean): Promise<PreferencesDto> {
  return invoke<PreferencesDto>("set_check_for_updates", { enabled });
}

/**
 * Opens a URL from a `ReleaseInfoDto` (the release page, or its
 * `SHA256SUMS.txt`) in the browser. `url` is forge-authored content
 * (GitHub's own API response) — the backend re-validates it is `https://
 * github.com/...` before ever opening anything; a mismatch resolves to
 * `false`, never a thrown error.
 */
export async function openUpdateLink(url: string): Promise<boolean> {
  return invoke<boolean>("open_update_link", { url });
}
