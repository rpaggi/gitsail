<script setup lang="ts">
// One inline-SVG icon component for the whole shell.
//
// Inline rather than an icon font or an SVG sprite file: the icons are
// stroke-based and inherit `currentColor`, which is what lets the same
// glyph sit on a blue nav pill and on a dark card without a second asset
// or a per-theme override. A single component (rather than ~25 one-path
// files) keeps the set visibly consistent — same 24-unit grid, same stroke
// width, same cap/join — because there is exactly one place that can drift.
//
// Icons are decorative here by default (`aria-hidden`): every icon in this
// app sits either next to a visible text label or inside a button that
// carries its own `aria-label`, so announcing the glyph too would just
// double up. `title` is offered for the rare standalone case.

import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    name: string;
    size?: number | string;
    /** Supplies an accessible name, turning the icon into an `img` role.
     * Only for an icon that is genuinely the sole carrier of meaning. */
    title?: string;
  }>(),
  { size: 16, title: undefined },
);

/** 24x24 stroke paths, keyed by glyph name. `null` fill, `currentColor`
 * stroke — set once on the `<svg>` so no path needs to repeat it. */
const PATHS: Record<string, string[]> = {
  home: ["M3 10.5 12 3l9 7.5", "M5.5 9.5V20h13V9.5", "M9.5 20v-6h5v6"],
  commit: ["M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7Z", "M12 3v5.5", "M12 15.5V21"],
  branch: [
    "M6.5 4.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M6.5 15.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M17.5 4.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M6.5 8.5v7",
    "M17.5 8.5c0 4-4.5 3.5-7 5.5",
  ],
  stash: [
    "M3.5 7.5h17v3h-17z",
    "M5 10.5V19a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-8.5",
    "M10 14.5h4",
  ],
  "pull-request": [
    "M6.5 5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M6.5 15a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M17.5 15a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
    "M6.5 9v6",
    "M17.5 15V9.5a2 2 0 0 0-2-2h-4",
    "M13.5 5.5 11.5 7.5l2 2",
  ],
  issue: ["M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17Z", "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z"],
  file: ["M13.5 3.5H7a1.5 1.5 0 0 0-1.5 1.5v14A1.5 1.5 0 0 0 7 20.5h10a1.5 1.5 0 0 0 1.5-1.5V8.5Z", "M13.5 3.5v5h5"],
  diff: ["M7 4.5v11", "M4.5 7h5", "M7 18.5h.01", "M14.5 7.5h5", "M17 5v5", "M14.5 17h5"],
  blame: [
    "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17Z",
    "M12 7.5V12l3 1.8",
  ],
  tag: [
    "M11.6 3.5H5a1.5 1.5 0 0 0-1.5 1.5v6.6a1.5 1.5 0 0 0 .44 1.06l7.4 7.4a1.5 1.5 0 0 0 2.12 0l6.6-6.6a1.5 1.5 0 0 0 0-2.12l-7.4-7.4A1.5 1.5 0 0 0 11.6 3.5Z",
    "M7.5 7.5h.01",
  ],
  remote: [
    "M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17Z",
    "M3.5 12h17",
    "M12 3.5c2.2 2.4 3.4 5.4 3.4 8.5s-1.2 6.1-3.4 8.5c-2.2-2.4-3.4-5.4-3.4-8.5S9.8 5.9 12 3.5Z",
  ],
  settings: [
    "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z",
    "M19.4 14.2a1.5 1.5 0 0 0 .3 1.65l.06.06a1.8 1.8 0 1 1-2.55 2.55l-.06-.06a1.5 1.5 0 0 0-1.65-.3 1.5 1.5 0 0 0-.9 1.37v.17a1.8 1.8 0 1 1-3.6 0v-.09a1.5 1.5 0 0 0-.98-1.37 1.5 1.5 0 0 0-1.65.3l-.06.06a1.8 1.8 0 1 1-2.55-2.55l.06-.06a1.5 1.5 0 0 0 .3-1.65 1.5 1.5 0 0 0-1.37-.9h-.17a1.8 1.8 0 1 1 0-3.6h.09a1.5 1.5 0 0 0 1.37-.98 1.5 1.5 0 0 0-.3-1.65l-.06-.06A1.8 1.8 0 1 1 8.2 4.66l.06.06a1.5 1.5 0 0 0 1.65.3h.07a1.5 1.5 0 0 0 .9-1.37v-.17a1.8 1.8 0 1 1 3.6 0v.09a1.5 1.5 0 0 0 .9 1.37 1.5 1.5 0 0 0 1.65-.3l.06-.06a1.8 1.8 0 1 1 2.55 2.55l-.06.06a1.5 1.5 0 0 0-.3 1.65v.07a1.5 1.5 0 0 0 1.37.9h.17a1.8 1.8 0 1 1 0 3.6h-.09a1.5 1.5 0 0 0-1.37.9Z",
  ],
  repo: [
    "M5.5 4.5A1.5 1.5 0 0 1 7 3h10.5v14H7a1.5 1.5 0 0 0-1.5 1.5Z",
    "M5.5 18.5A1.5 1.5 0 0 0 7 20h10.5v-3",
  ],
  search: ["M11 4.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13Z", "M15.8 15.8 20 20"],
  refresh: ["M20 5.5v5h-5", "M4 18.5v-5h5", "M19.2 10.5a7.5 7.5 0 0 0-12.6-3L4 10.5", "M4.8 13.5a7.5 7.5 0 0 0 12.6 3l2.6-3"],
  sync: ["M4.5 9.5h11l-3-3", "M19.5 14.5h-11l3 3"],
  chevron: ["m7 10 5 5 5-5"],
  check: ["M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17Z", "m8.2 12.2 2.6 2.6 5-5.4"],
  "arrow-up": ["M12 20V4.5", "m5.5 11 6.5-6.5 6.5 6.5"],
  "arrow-down": ["M12 4v15.5", "m5.5 13 6.5 6.5 6.5-6.5"],
  "arrow-right": ["M4.5 12h15", "m13 5.5 6.5 6.5-6.5 6.5"],
  kebab: ["M12 5.5h.01", "M12 12h.01", "M12 18.5h.01"],
  alert: ["M12 4 2.8 20h18.4Z", "M12 10v4", "M12 17h.01"],
  folder: ["M3.5 6.5A1.5 1.5 0 0 1 5 5h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5Z"],
  external: ["M14 4.5h5.5V10", "M19.5 4.5 11 13", "M17.5 14v4.5a1.5 1.5 0 0 1-1.5 1.5H6a1.5 1.5 0 0 1-1.5-1.5V8A1.5 1.5 0 0 1 6 6.5h4.5"],
  anchor: ["M12 6.5a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z", "M12 6.5V21", "M4 13.5a8 8 0 0 0 16 0", "M4 13.5h3", "M20 13.5h-3"],
};

const paths = computed(() => PATHS[props.name] ?? PATHS.issue);
const dimension = computed(() => (typeof props.size === "number" ? `${props.size}px` : props.size));
</script>

<template>
  <svg
    class="gs-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    :width="dimension"
    :height="dimension"
    :role="title ? 'img' : undefined"
    :aria-hidden="title ? undefined : 'true'"
    focusable="false"
  >
    <title v-if="title">{{ title }}</title>
    <path v-for="(d, i) in paths" :key="i" :d="d" />
  </svg>
</template>

<style scoped>
.gs-icon {
  display: block;
  flex: none;
}
</style>
