<script setup lang="ts">
// The rounded surface every panel in the redesigned shell sits on.
//
// One component rather than a `.card` class so the header is structural:
// a card always renders its title as a real heading at a caller-chosen
// level, which is what keeps the document outline correct as cards get
// nested (an Overview page of cards, each with its own sub-sections)
// instead of every panel independently guessing between `h2` and `h3`.
//
// `bleed` exists because two of the mockup's cards (the commit list and
// the diff) run their content edge-to-edge under the header, while the
// rest are padded — so padding belongs to the card, not to every panel.

withDefaults(
  defineProps<{
    title?: string;
    /** Heading level for `title`. */
    as?: "h2" | "h3";
    /** Drop the body padding, for content that draws its own rows. */
    bleed?: boolean;
    /** Let the body scroll and fill the available height. */
    fill?: boolean;
  }>(),
  { title: undefined, as: "h2", bleed: false, fill: false },
);
</script>

<template>
  <section class="gs-card" :class="{ 'gs-card--fill': fill }">
    <header v-if="title || $slots.actions" class="gs-card__header">
      <component :is="as" v-if="title" class="gs-card__title">{{ title }}</component>
      <div v-if="$slots.actions" class="gs-card__actions">
        <slot name="actions" />
      </div>
    </header>
    <div class="gs-card__body" :class="{ 'gs-card__body--bleed': bleed, 'gs-card__body--fill': fill }">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.gs-card {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.gs-card--fill {
  flex: 1;
}

.gs-card__header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--color-border);
  flex: none;
}

.gs-card__title {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 650;
  color: var(--color-text);
  /* Takes the slack so `actions` is pinned right without the caller
     needing a spacer element. */
  flex: 1;
  min-width: 0;
}

.gs-card__actions {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  flex: none;
}

.gs-card__body {
  padding: 0.9rem 1rem;
  min-height: 0;
}

.gs-card__body--bleed {
  padding: 0;
}

.gs-card__body--fill {
  flex: 1;
  overflow: auto;
}
</style>
