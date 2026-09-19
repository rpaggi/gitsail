<script setup lang="ts">
// "File Changes": a compact, read-only unified diff of the working tree,
// for the Overview's right rail.
//
// This is a *preview*, not a replacement for `DiffViewer.vue` — it shows
// one file at a time with no staging, no side-by-side toggle and no hunk
// selection, because the rail is ~380px wide and any of those would make
// it unusable rather than merely small. The Diff view remains where the
// real work happens, and "Open in Diff" is the link between them.
//
// The numbers in the footer are counted from the hunks actually loaded,
// so a truncated diff reports what it has rather than a total it cannot
// substantiate.

import { computed, onMounted, ref } from "vue";

import GsCard from "./GsCard.vue";
import GsIcon from "./GsIcon.vue";
import { unifiedLines } from "./diffPresentation";
import { useDiffStore } from "../stores/diff";
import { useRepositorySessionStore } from "../stores/session";

const emit = defineEmits<{ (event: "open-diff"): void }>();

const diff = useDiffStore();
const session = useRepositorySessionStore();

/** Which of the loaded files the preview is showing. An index rather than
 * a path so it survives the diff being reloaded with different contents. */
const fileIndex = ref(0);

const files = computed(() => diff.diff?.files ?? []);

const current = computed(() => files.value[Math.min(fileIndex.value, files.value.length - 1)] ?? null);

/** Hunks, capped. A single large file can carry hundreds of hunks, and
 * this card is a preview in a narrow rail — rendering them all would cost
 * more than it shows. */
const MAX_HUNKS = 4;

const hunks = computed(() =>
  (current.value?.hunks ?? []).slice(0, MAX_HUNKS).map((hunk) => ({
    header: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    lines: unifiedLines(hunk),
  })),
);

const truncatedHunks = computed(() => Math.max(0, (current.value?.hunks.length ?? 0) - MAX_HUNKS));

const counts = computed(() => {
  let additions = 0;
  let deletions = 0;
  for (const file of files.value) {
    for (const hunk of file.hunks) {
      for (const line of hunk.lines) {
        if (line.origin === "addition") {
          additions += 1;
        } else if (line.origin === "deletion") {
          deletions += 1;
        }
      }
    }
  }
  return { additions, deletions, files: files.value.length };
});

function nextFile(): void {
  if (files.value.length > 0) {
    fileIndex.value = (fileIndex.value + 1) % files.value.length;
  }
}

onMounted(() => {
  // Seed the card with the whole unstaged working-tree diff when nothing
  // else has populated the store yet. Guarded so it never clobbers a diff
  // the person deliberately opened for one file from another view.
  if (session.repository && diff.diff === null && !diff.isLoading) {
    void diff.load(null, false);
  }
});
</script>

<template>
  <GsCard title="File Changes" as="h3">
    <template #actions>
      <button
        v-if="files.length > 1"
        type="button"
        class="btn-ghost file-changes__next"
        :aria-label="`Show next changed file (${fileIndex + 1} of ${files.length})`"
        @click="nextFile"
      >
        {{ fileIndex + 1 }}/{{ files.length }}
        <GsIcon name="arrow-right" :size="13" />
      </button>
      <button type="button" class="btn-ghost file-changes__open" @click="emit('open-diff')">
        Open in Diff
      </button>
    </template>

    <p v-if="diff.lastError" class="error" role="alert">{{ diff.lastError.message }}</p>
    <p v-else-if="diff.isLoading" class="file-changes__empty" role="status">Loading diff&hellip;</p>
    <p v-else-if="!current" class="file-changes__empty">
      No uncommitted changes. Edit a file and it will show up here.
    </p>

    <template v-else>
      <p class="file-changes__path" :title="current.path">{{ current.path }}</p>

      <p v-if="current.isBinary" class="file-changes__empty">Binary file — no line diff to show.</p>

      <div v-else class="file-changes__diff">
        <template v-for="(hunk, hunkIndex) in hunks" :key="hunkIndex">
          <p class="file-changes__hunk">{{ hunk.header }}</p>
          <p
            v-for="(line, lineIndex) in hunk.lines"
            :key="`${hunkIndex}-${lineIndex}`"
            class="file-changes__line"
            :class="`file-changes__line--${line.origin}`"
          >
            <span class="file-changes__lineno" aria-hidden="true">{{ line.oldLineNumber ?? "" }}</span>
            <span class="file-changes__lineno" aria-hidden="true">{{ line.newLineNumber ?? "" }}</span>
            <!-- The +/- marker is real text, so an added line is not
                 distinguished by its green tint alone. -->
            <span class="file-changes__sign">{{
              line.origin === "addition" ? "+" : line.origin === "deletion" ? "-" : " "
            }}</span>
            <span class="file-changes__code">{{ line.content }}</span>
          </p>
        </template>
        <p v-if="truncatedHunks > 0" class="file-changes__more">
          +{{ truncatedHunks }} more hunk{{ truncatedHunks === 1 ? "" : "s" }} — open in Diff to see them all.
        </p>
      </div>

      <p class="file-changes__footer">
        <span
          >{{ counts.files }} file{{ counts.files === 1 ? "" : "s" }} changed</span
        >
        <span class="file-changes__added">+{{ counts.additions }}</span>
        <span class="file-changes__removed">&minus;{{ counts.deletions }}</span>
      </p>
    </template>
  </GsCard>
</template>

<style scoped>
.file-changes__next,
.file-changes__open {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.76rem;
  padding: 0.2rem 0.45rem;
  border: 1px solid var(--color-border-strong);
  background: var(--color-surface-raised);
  color: var(--color-text-muted);
}

.file-changes__empty {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 0.82rem;
}

.file-changes__path {
  margin: 0 0 0.4rem;
  font-family: var(--font-mono);
  font-size: 0.78rem;
  color: var(--color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-changes__diff {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  overflow: auto;
  max-height: 15rem;
  background: var(--color-bg);
}

.file-changes__hunk,
.file-changes__line,
.file-changes__more {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 0.72rem;
  line-height: 1.55;
  white-space: pre;
  display: flex;
}

.file-changes__hunk {
  padding: 0.15rem 0.5rem;
  color: var(--color-accent);
  background: var(--color-surface-alt);
}

.file-changes__more {
  padding: 0.25rem 0.5rem;
  color: var(--color-text-faint);
  white-space: normal;
}

.file-changes__line--addition {
  background: var(--color-diff-add-bg);
  color: var(--color-diff-add-text);
}

.file-changes__line--deletion {
  background: var(--color-diff-del-bg);
  color: var(--color-diff-del-text);
}

.file-changes__lineno {
  flex: none;
  width: 2.1rem;
  padding-right: 0.3rem;
  text-align: right;
  color: var(--color-text-faint);
  user-select: none;
}

.file-changes__sign {
  flex: none;
  width: 1ch;
  user-select: none;
}

.file-changes__code {
  flex: 1;
  padding-left: 0.4rem;
}

.file-changes__footer {
  display: flex;
  gap: 0.6rem;
  margin: 0.55rem 0 0;
  font-size: 0.76rem;
  color: var(--color-text-muted);
}

.file-changes__added {
  color: var(--color-success);
  font-weight: 600;
}

.file-changes__removed {
  color: var(--color-danger);
  font-weight: 600;
}
</style>
