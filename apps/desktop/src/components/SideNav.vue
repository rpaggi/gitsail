<script setup lang="ts">
// The left sidebar: the app's primary view switcher, plus a summary of the
// repository it is switching views *for*.
//
// Accessibility: a `<nav>` landmark containing a list of buttons, with the
// active one marked `aria-current="page"` — the switcher changes what the
// `<main>` region shows without navigating, so `aria-current` (not
// `aria-selected`) is what describes it. Keyboard handling keeps the
// roving-tabindex behavior the previous tablist shell had: Tab reaches the
// nav once and lands on the active item, Up/Down/Home/End move within it.
// Without that, Tab would stop on all twelve items before ever reaching
// the content they control.

import { computed, nextTick, ref, watch } from "vue";

import GsIcon from "./GsIcon.vue";
import { rovingNextIndex } from "./keyboardNav";
import { NAV_ITEMS, type ViewId } from "./navigation";
import { useRepositorySessionStore } from "../stores/session";

const props = defineProps<{ active: ViewId }>();
const emit = defineEmits<{ (event: "select", id: ViewId): void }>();

const session = useRepositorySessionStore();

const buttonEls = ref<(HTMLButtonElement | null)[]>([]);

function setButtonRef(el: Element | null, index: number): void {
  buttonEls.value[index] = el as HTMLButtonElement | null;
}

const activeIndex = computed(() =>
  Math.max(
    0,
    NAV_ITEMS.findIndex((item) => item.id === props.active),
  ),
);

/** Which item is currently tabbable. Tracked separately from `active` so
 * arrow keys can move focus through the list without committing to a view
 * on every keypress — activation stays on Enter/Space (the button's own
 * default), matching how a native list behaves. */
const focusIndex = ref(activeIndex.value);

// Keep the tab stop on the active item when the view changes from
// somewhere else (a fallback to Settings with no repository open, say), so
// Tab never lands on a stale row.
watch(activeIndex, (index) => {
  focusIndex.value = index;
});

function onKeydown(event: KeyboardEvent, index: number): void {
  const next = rovingNextIndex(index, event.key, NAV_ITEMS.length, "vertical");
  if (next === null) {
    return;
  }
  event.preventDefault();
  focusIndex.value = next;
  void nextTick(() => buttonEls.value[next]?.focus());
}

const repositoryName = computed<string | null>(() => {
  const root = session.repository?.rootPath;
  if (!root) {
    return null;
  }
  const segments = root.split(/[/\\]+/).filter((segment) => segment.length > 0);
  return segments.length > 0 ? segments[segments.length - 1] : root;
});

/**
 * The working-tree summary shown at the foot of the sidebar.
 *
 * `null` while the status has not been read yet — rendered as "Checking…"
 * rather than optimistically as "clean", which would be a claim the app
 * cannot back up and the single most damaging thing to get wrong here.
 */
const worktree = computed(() => {
  const status = session.status;
  if (!status) {
    return null;
  }
  return { isClean: status.isClean, fileCount: status.files.length };
});
</script>

<template>
  <nav class="sidenav" aria-label="Main">
    <ul class="sidenav__list">
      <li v-for="(item, index) in NAV_ITEMS" :key="item.id">
        <button
          :ref="(el) => setButtonRef(el as Element | null, index)"
          type="button"
          class="sidenav__item"
          :class="{ 'sidenav__item--active': item.id === active }"
          :aria-current="item.id === active ? 'page' : undefined"
          :tabindex="index === focusIndex ? 0 : -1"
          @click="emit('select', item.id)"
          @focus="focusIndex = index"
          @keydown="onKeydown($event, index)"
        >
          <GsIcon :name="item.icon" :size="17" />
          <span class="sidenav__label">{{ item.label }}</span>
          <!-- Named in text, not by a color or a dimmed style alone, so
               "this one is not built yet" survives both themes and a
               screen reader. -->
          <span v-if="!item.available" class="sidenav__soon">soon</span>
        </button>
      </li>
    </ul>

    <div v-if="session.repository" class="sidenav__footer">
      <div class="sidenav__repo">
        <GsIcon name="repo" :size="16" />
        <span class="sidenav__repo-text">
          <span class="sidenav__repo-name">{{ repositoryName }}</span>
          <span class="sidenav__repo-branch">{{ session.repository.currentBranch ?? "detached" }}</span>
        </span>
        <span
          class="sidenav__dot"
          :class="worktree?.isClean ? 'sidenav__dot--clean' : 'sidenav__dot--dirty'"
          aria-hidden="true"
        />
      </div>

      <p v-if="worktree === null" class="sidenav__status" role="status">Checking working tree&hellip;</p>
      <p v-else-if="worktree.isClean" class="sidenav__status sidenav__status--clean" role="status">
        <GsIcon name="check" :size="13" />
        Working tree clean
      </p>
      <p v-else class="sidenav__status sidenav__status--dirty" role="status">
        <GsIcon name="alert" :size="13" />
        {{ worktree.fileCount }} file{{ worktree.fileCount === 1 ? "" : "s" }} changed
      </p>
    </div>
  </nav>
</template>

<style scoped>
.sidenav {
  grid-area: sidenav;
  display: flex;
  flex-direction: column;
  width: 208px;
  padding: 0.6rem;
  background: var(--color-chrome);
  border-right: 1px solid var(--color-border);
  min-height: 0;
}

.sidenav__list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  /* The nav is the part that gives way when the window is short — the
     repository footer below stays pinned and visible. */
  overflow-y: auto;
  min-height: 0;
  flex: 1;
}

.sidenav__item {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  width: 100%;
  height: 36px;
  padding: 0 0.6rem;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  background: transparent;
  color: var(--color-text-muted);
  font-size: 0.875rem;
  font-weight: 500;
  cursor: pointer;
  text-align: left;
}

.sidenav__item:hover {
  background: var(--color-surface-alt);
  border-color: transparent;
  color: var(--color-text);
}

.sidenav__item--active,
.sidenav__item--active:hover {
  background: var(--color-accent-solid);
  border-color: var(--color-accent-solid);
  color: var(--color-accent-contrast);
  font-weight: 600;
}

.sidenav__label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidenav__soon {
  flex: none;
  font-size: 0.62rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 0.05rem 0.3rem;
  border-radius: var(--radius-pill);
  background: var(--color-surface-alt);
  color: var(--color-text-faint);
}

.sidenav__item--active .sidenav__soon {
  background: rgba(255, 255, 255, 0.2);
  color: var(--color-accent-contrast);
}

.sidenav__footer {
  flex: none;
  margin-top: 0.6rem;
  padding-top: 0.6rem;
  border-top: 1px solid var(--color-border);
}

.sidenav__repo {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-text-muted);
}

.sidenav__repo-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  line-height: 1.25;
}

.sidenav__repo-name {
  color: var(--color-text);
  font-size: 0.82rem;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidenav__repo-branch {
  font-size: 0.72rem;
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidenav__dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.sidenav__dot--clean {
  background: var(--color-success);
}

.sidenav__dot--dirty {
  background: var(--color-warning);
}

.sidenav__status {
  display: flex;
  align-items: center;
  gap: 0.3rem;
  margin: 0.45rem 0 0.15rem;
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.sidenav__status--clean {
  color: var(--color-success);
}

.sidenav__status--dirty {
  color: var(--color-warning);
}

/* Stacked layout: the sidebar becomes a horizontal strip of items above
   the content rather than a column beside it. */
@media (max-width: 60rem) {
  .sidenav {
    width: auto;
    border-right: none;
    border-bottom: 1px solid var(--color-border);
  }
  .sidenav__list {
    flex-direction: row;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .sidenav__item {
    width: auto;
    white-space: nowrap;
  }
  .sidenav__footer {
    display: none;
  }
}
</style>
