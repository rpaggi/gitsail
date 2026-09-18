<script setup lang="ts">
// Desktop update checking (T-260/US-127). Check-only, never an
// auto-installer — see `gitsail_application::update_check`'s own module
// doc comment (mirrored by `stores/update.ts`) for the full rationale
// ADR-023 (GitHub-Releases-only distribution, no code signing) requires.
// This component only ever shows "here is what GitHub's latest release
// is" plus a link the person opens and verifies themselves; nothing here
// downloads or installs anything.
//
// `release.notes` is forge-authored content (GitHub's own release body) —
// rendered only through plain `{{ }}` text interpolation below, exactly
// like `PullRequestsPanel.vue` already treats a PR title/author: never
// `v-html`, never interpreted as Markdown/HTML.

import { onMounted } from "vue";

import { usePreferencesStore } from "../stores/preferences";
import { useUpdateStore } from "../stores/update";

const preferences = usePreferencesStore();
const update = useUpdateStore();

onMounted(() => {
  // US-127's own "no unsolicited network call" convention: this is a
  // single, non-looping attempt, gated server-side by the
  // check-for-updates preference and a once-per-day throttle
  // (`gitsail_application::update_check::ONE_DAY_SECONDS`) — see
  // `CheckForUpdate::execute`'s own doc comment. A disabled preference or
  // a too-recent previous check both make this a no-op (the `"skipped"`
  // outcome), never a second call.
  void update.check("automatic");
});

function checkNow(): void {
  void update.check("manual");
}

function openLink(url: string | undefined): void {
  if (!url) return;
  void update.openLink(url);
}
</script>

<template>
  <div class="update-checker">
    <div class="update-checker__row">
      <button type="button" :disabled="update.isChecking" @click="checkNow">
        {{ update.isChecking ? "Checking…" : "Check for updates" }}
      </button>
      <label class="update-checker__toggle">
        <input
          type="checkbox"
          :checked="preferences.checkForUpdates"
          @change="preferences.setCheckForUpdates(($event.target as HTMLInputElement).checked)"
        />
        Check automatically
      </label>
    </div>

    <p v-if="update.outcome?.state === 'upToDate'" class="update-checker__status" role="status">
      You're up to date ({{ update.outcome.currentTag }}).
    </p>

    <div
      v-else-if="update.outcome?.state === 'updateAvailable'"
      class="update-checker__available"
      role="status"
    >
      <p>
        GitSail {{ update.outcome.release.tag }} is available (you have
        {{ update.outcome.currentTag }}).
      </p>
      <p v-if="update.outcome.release.notes" class="update-checker__notes">
        {{ update.outcome.release.notes }}
      </p>
      <div class="update-checker__links">
        <button type="button" @click="openLink(update.outcome.release.htmlUrl)">
          Open release page
        </button>
        <button
          v-if="update.outcome.release.checksumsUrl"
          type="button"
          @click="openLink(update.outcome.release.checksumsUrl)"
        >
          Open SHA256SUMS.txt
        </button>
      </div>
      <p class="update-checker__hint">
        Download and install this update yourself from the release page; verify the download
        against SHA256SUMS.txt. GitSail does not download or install updates automatically.
      </p>
    </div>

    <div
      v-else-if="update.outcome?.state === 'cannotDetermineCurrentVersion'"
      class="update-checker__hint-block"
      role="status"
    >
      <p>
        A release ({{ update.outcome.release.tag }}) is published, but this build's own version
        could not be determined (a development build).
      </p>
      <button type="button" @click="openLink(update.outcome.release.htmlUrl)">
        Open release page to compare manually
      </button>
    </div>

    <p
      v-else-if="update.outcome?.state === 'checkFailed'"
      class="update-checker__error"
      role="alert"
    >
      Could not check for updates: {{ update.outcome.error.message }}. You can try again later.
    </p>

    <p
      v-else-if="update.outcome?.state === 'noReleasesPublished'"
      class="update-checker__status"
      role="status"
    >
      No releases have been published yet.
    </p>

    <p v-if="update.lastError" class="update-checker__error" role="alert">
      Could not check for updates: {{ update.lastError.message }}
    </p>
  </div>
</template>

<style scoped>
.update-checker {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.update-checker__row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}
.update-checker__row button {
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0.3rem 0.75rem;
  color: var(--color-text);
  cursor: pointer;
}
.update-checker__toggle {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  color: var(--color-text-muted);
  font-size: 0.85rem;
}
.update-checker__status {
  color: var(--color-text-muted);
  font-size: 0.8rem;
  margin: 0;
}
.update-checker__available,
.update-checker__hint-block {
  border: 1px solid var(--color-accent);
  border-radius: 4px;
  padding: 0.5rem;
  font-size: 0.85rem;
}
.update-checker__notes {
  white-space: pre-wrap;
  color: var(--color-text-muted);
  font-size: 0.8rem;
}
.update-checker__links {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
  margin: 0.35rem 0;
}
.update-checker__links button {
  background: var(--color-surface-alt);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 0.25rem 0.6rem;
  color: var(--color-text);
  cursor: pointer;
}
.update-checker__hint {
  color: var(--color-text-muted);
  font-size: 0.75rem;
  margin: 0;
}
.update-checker__error {
  color: var(--color-danger);
  font-size: 0.8rem;
  margin: 0;
}
</style>
