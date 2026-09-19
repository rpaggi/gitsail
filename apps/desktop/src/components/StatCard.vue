<script setup lang="ts">
// One tile in the Overview's summary row: a tinted icon, a large value and
// a label naming what the value measures.
//
// `value` is a string, not a number, because two of the four tiles are not
// numeric (a branch name, "Clean") and because an unavailable figure must
// be able to render as an em dash rather than as a `0` — claiming "0
// commits ahead" when the app has not read the upstream yet is the exact
// kind of confident-but-wrong that makes a Git client untrustworthy.

import GsIcon from "./GsIcon.vue";

withDefaults(
  defineProps<{
    icon: string;
    value: string;
    label: string;
    tone?: "accent" | "success" | "warning" | "danger" | "muted";
    /** Explains an unavailable or surprising value on hover. */
    hint?: string;
  }>(),
  { tone: "accent", hint: undefined },
);
</script>

<template>
  <div class="stat-card" :class="`stat-card--${tone}`" :title="hint">
    <span class="stat-card__icon" aria-hidden="true">
      <GsIcon :name="icon" :size="18" />
    </span>
    <span class="stat-card__text">
      <span class="stat-card__value">{{ value }}</span>
      <span class="stat-card__label">{{ label }}</span>
    </span>
  </div>
</template>

<style scoped>
.stat-card {
  display: flex;
  align-items: center;
  gap: 0.7rem;
  padding: 0.75rem 0.9rem;
  background: var(--color-surface-raised);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  min-width: 0;
}

.stat-card__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: var(--radius-md);
  flex: none;
}

.stat-card--accent .stat-card__icon {
  background: var(--color-accent-soft-strong);
  color: var(--color-accent);
}
.stat-card--success .stat-card__icon {
  background: var(--color-diff-add-bg);
  color: var(--color-success);
}
.stat-card--warning .stat-card__icon {
  background: var(--color-warning-contrast);
  color: var(--color-warning);
}
.stat-card--danger .stat-card__icon {
  background: var(--color-diff-del-bg);
  color: var(--color-danger);
}
.stat-card--muted .stat-card__icon {
  background: var(--color-surface-alt);
  color: var(--color-text-muted);
}

.stat-card__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.25;
}

.stat-card__value {
  font-size: 1.15rem;
  font-weight: 700;
  color: var(--color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stat-card__label {
  font-size: 0.76rem;
  color: var(--color-text-muted);
}
</style>
