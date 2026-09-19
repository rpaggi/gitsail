<script setup lang="ts">
// An author's avatar: initials on a deterministic color (see
// `authorAvatar.ts` for why this is derived rather than fetched).
//
// The circle itself is `aria-hidden` and the author's real name goes in a
// `.sr-only` span, because "AL" read aloud is noise — the point of the
// initials is visual recognition, and assistive tech should get the name
// the initials stand for.

import { computed } from "vue";

import { authorColorIndex, authorInitials } from "./authorAvatar";
import type { SignatureDto } from "../services/dto";

const props = withDefaults(
  defineProps<{ author: SignatureDto; size?: number }>(),
  { size: 28 },
);

const initials = computed(() => authorInitials(props.author));
const colorIndex = computed(() => authorColorIndex(props.author));
const label = computed(() => props.author.name || props.author.email || "Unknown author");
</script>

<template>
  <span class="gs-avatar" :title="label">
    <span
      class="gs-avatar__disc"
      :class="`gs-avatar__disc--${colorIndex}`"
      :style="{ width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.38)}px` }"
      aria-hidden="true"
      >{{ initials }}</span
    >
    <span class="sr-only">{{ label }}</span>
  </span>
</template>

<style scoped>
.gs-avatar {
  display: inline-flex;
  flex: none;
}

.gs-avatar__disc {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-weight: 700;
  letter-spacing: 0.01em;
  /* Dark ink rather than white: the avatar ramp is deliberately light
     (see `--color-avatar-*` in theme.css), which is what a disc needs to
     carry legible initials. */
  color: var(--color-avatar-ink);
  user-select: none;
}

.gs-avatar__disc--0 {
  background: var(--color-avatar-1);
}
.gs-avatar__disc--1 {
  background: var(--color-avatar-2);
}
.gs-avatar__disc--2 {
  background: var(--color-avatar-3);
}
.gs-avatar__disc--3 {
  background: var(--color-avatar-4);
}
.gs-avatar__disc--4 {
  background: var(--color-avatar-5);
}
.gs-avatar__disc--5 {
  background: var(--color-avatar-6);
}
</style>
