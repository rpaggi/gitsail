<script setup lang="ts">
// Tags/remotes/stash sidebar panel (T-195/US-062 criterion 2), mirroring
// `gitsail-tui`'s own `Panel::References`/`ReferenceView` (T-183,
// `crates/gitsail-tui/src/{app,ui}.rs`): one panel, three sub-views a
// person switches between, each with its own explicit empty/error/loading
// state (criterion 3) rather than one bare list.
//
// Remotes reuse `stores/sync.ts`'s own `remotes` field/`loadRemotes()` —
// not refetched here — since `SyncPanel.vue` already owns that read; tags
// and stash are this story's own `stores/references.ts`. Every read is
// read-only: `services/references.ts` never calls a mutating command
// (T-195 criterion 3).
//
// The sub-view switcher follows `AppShell.vue`'s own tablist pattern
// (WAI-ARIA APG "tabs", roving tabindex via `keyboardNav.ts`'s
// `rovingNextIndex`) for consistency with the one other multi-section
// control in this app.

import { computed, nextTick, onMounted, ref } from "vue";

import { rovingNextIndex } from "./keyboardNav";
import { formatGitTimestamp } from "./timestampFormat";
import { NO_REMOTES_TEXT, NO_STASH_ENTRIES_TEXT, NO_TAGS_TEXT } from "./referencesPresentation";
import { useReferencesStore } from "../stores/references";
import { useSyncStore } from "../stores/sync";

const references = useReferencesStore();
const sync = useSyncStore();

interface SubView {
  id: "tags" | "remotes" | "stash";
  label: string;
}

const subViews: SubView[] = [
  { id: "tags", label: "Tags" },
  { id: "remotes", label: "Remotes" },
  { id: "stash", label: "Stash" },
];

// The shell's sidebar offers Tags, Remotes and Stashes as three separate
// destinations, so it needs to say which one this panel should open on.
// Only the *initial* sub-view: once mounted the tablist owns the choice,
// so switching tabs inside the panel is not fought by the prop.
const props = withDefaults(
  defineProps<{ initialView?: SubView["id"] }>(),
  { initialView: "tags" },
);

const activeIndex = ref(Math.max(0, subViews.findIndex((view) => view.id === props.initialView)));
const activeView = computed(() => subViews[activeIndex.value].id);
const tabButtonEls = ref<(HTMLButtonElement | null)[]>([]);

function setTabButtonRef(el: Element | null, index: number): void {
  tabButtonEls.value[index] = el as HTMLButtonElement | null;
}

function selectView(index: number): void {
  activeIndex.value = index;
}

function onTabKeydown(event: KeyboardEvent, index: number): void {
  const next = rovingNextIndex(index, event.key, subViews.length, "horizontal");
  if (next === null) {
    return;
  }
  event.preventDefault();
  activeIndex.value = next;
  void nextTick(() => {
    tabButtonEls.value[next]?.focus();
  });
}

onMounted(() => {
  void references.loadAll();
  // `SyncPanel.vue` also calls this on its own mount — both mounted
  // sections converge on the same `stores/sync.ts` state, so whichever
  // mounts first populates it for the other (Pinia stores are singletons).
  void sync.loadRemotes();
});
</script>

<template>
  <section class="references-panel" aria-label="Tags, remotes and stash">

    <div class="references-panel__tablist" role="tablist" aria-label="Tags, remotes, stash">
      <button
        v-for="(view, index) in subViews"
        :key="view.id"
        :ref="(el) => setTabButtonRef(el as Element | null, index)"
        role="tab"
        type="button"
        :id="`references-tab-${view.id}`"
        :aria-selected="activeIndex === index"
        :aria-controls="`references-tabpanel-${view.id}`"
        :tabindex="activeIndex === index ? 0 : -1"
        @click="selectView(index)"
        @keydown="onTabKeydown($event, index)"
      >
        {{ view.label }}
      </button>
    </div>

    <div
      v-show="activeView === 'tags'"
      id="references-tabpanel-tags"
      role="tabpanel"
      aria-labelledby="references-tab-tags"
      tabindex="0"
    >
      <p v-if="references.tagsError" class="error" role="alert">{{ references.tagsError.message }}</p>
      <p v-else-if="references.isLoadingTags" role="status">Loading tags&hellip;</p>
      <p v-else-if="references.tags.length === 0">{{ NO_TAGS_TEXT }}</p>
      <ul v-else class="references-panel__list">
        <li v-for="tag in references.tags" :key="tag.name">
          <strong>{{ tag.name }}</strong>
          <span class="references-panel__muted"> -&gt; {{ tag.target.slice(0, 8) }}</span>
          <span class="references-panel__badge">{{ tag.kind.kind }}</span>
          <span v-if="tag.kind.kind === 'annotated'" class="references-panel__muted">
            {{ tag.kind.message }} — {{ tag.kind.tagger.name }}, {{ formatGitTimestamp(tag.kind.date) }}
          </span>
        </li>
      </ul>
    </div>

    <div
      v-show="activeView === 'remotes'"
      id="references-tabpanel-remotes"
      role="tabpanel"
      aria-labelledby="references-tab-remotes"
      tabindex="0"
    >
      <p v-if="sync.remotes.length === 0 && !sync.isLoadingRemotes">{{ NO_REMOTES_TEXT }}</p>
      <p v-else-if="sync.isLoadingRemotes" role="status">Loading remotes&hellip;</p>
      <ul v-else class="references-panel__list">
        <li v-for="remote in sync.remotes" :key="remote.name">
          <strong>{{ remote.name }}</strong>
          <span class="references-panel__muted"> fetch={{ remote.fetchUrl }} push={{ remote.pushUrl }}</span>
        </li>
      </ul>
    </div>

    <div
      v-show="activeView === 'stash'"
      id="references-tabpanel-stash"
      role="tabpanel"
      aria-labelledby="references-tab-stash"
      tabindex="0"
    >
      <p v-if="references.stashesError" class="error" role="alert">{{ references.stashesError.message }}</p>
      <p v-else-if="references.isLoadingStashes" role="status">Loading stash&hellip;</p>
      <p v-else-if="references.stashes.length === 0">{{ NO_STASH_ENTRIES_TEXT }}</p>
      <ul v-else class="references-panel__list">
        <li v-for="entry in references.stashes" :key="entry.index">
          <strong>stash@{{ '{' }}{{ entry.index }}{{ '}' }}</strong>
          <span class="references-panel__muted">
            {{ entry.commit.slice(0, 8) }} {{ entry.message }} — {{ formatGitTimestamp(entry.date) }}
          </span>
        </li>
      </ul>
    </div>
  </section>
</template>

<style scoped>
.references-panel__tablist {
  display: flex;
  gap: 0.25rem;
  margin-bottom: 0.5rem;
  border-bottom: 1px solid var(--color-border);
}
.references-panel__tablist button {
  background: none;
  border: none;
  padding: 0.3rem 0.6rem;
  color: var(--color-text-muted);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  font-size: 0.85rem;
}
.references-panel__tablist button[aria-selected="true"] {
  color: var(--color-text);
  border-bottom-color: var(--color-accent);
  font-weight: 600;
}
.references-panel__list {
  list-style: none;
  margin: 0;
  padding: 0;
  font-size: 0.8rem;
}
.references-panel__list li {
  padding: 0.2rem 0;
  border-bottom: 1px solid var(--color-border);
  display: flex;
  flex-wrap: wrap;
  gap: 0.3rem;
  align-items: baseline;
}
.references-panel__muted {
  color: var(--color-text-muted);
}
.references-panel__badge {
  font-size: 0.7rem;
  padding: 0.05rem 0.35rem;
  border-radius: 3px;
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  color: var(--color-text-muted);
}
.error {
  color: var(--color-danger, #c0392b);
}
</style>
