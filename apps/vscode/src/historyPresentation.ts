// Pure presentation helpers turning DTOs into `QuickPickItemLike[]` for
// T-207 (file history) and T-208 (line history) — no `vscode` import,
// unit-testable as plain data transforms; `historyController.ts` is the
// only caller, and only `extension.ts` ever turns a `QuickPickItemLike`
// into a real `vscode.QuickPickItem`.

import { formatGitTimestamp } from "./blameFormat";
import { CommitDto, LineHistoryDto } from "./dto";
import { FileHistoryOutcome } from "./fileHistoryService";
import { QuickPickItemLike } from "./historyHostTypes";

/** Synthetic item ids this module can produce, distinct from any real
 * commit hash (a 40-hex-character string could never collide with these). */
export const LOAD_MORE_ITEM_ID = "__gitsail_load_more__";
export const NO_HISTORY_ITEM_ID = "__gitsail_no_history__";

function commitQuickPickItem(commit: CommitDto): QuickPickItemLike {
  return {
    id: commit.hash,
    label: `${commit.shortHash}  ${firstLineOf(commit.subject)}`,
    description: commit.author.name,
    detail: formatGitTimestamp(commit.authorDate),
  };
}

function firstLineOf(text: string): string {
  const index = text.indexOf("\n");
  return index === -1 ? text : text.slice(0, index);
}

/**
 * Builds the picker rows for one page of file history (US-074 criteria 1
 * and 3). An empty page never renders as an empty, unexplained list — a
 * dedicated "no history" row makes the state explicit; a "Load more…" row
 * is appended whenever the CLI reports more pages exist, carrying the
 * pagination cursor along (US-074 criterion 1: "lista paginada preserva
 * contexto").
 */
export function buildFileHistoryQuickPickItems(outcome: FileHistoryOutcome): QuickPickItemLike[] {
  if (outcome.kind === "empty") {
    return [
      {
        id: NO_HISTORY_ITEM_ID,
        label: "No history for this file",
        detail: "GitSail found no commits touching this path.",
      },
    ];
  }
  const items = outcome.page.items.map(commitQuickPickItem);
  if (outcome.page.hasMore) {
    items.push({
      id: LOAD_MORE_ITEM_ID,
      label: "Load more…",
      description: outcome.page.nextCursor,
    });
  }
  return items;
}

/** Builds the picker rows for a line-history result (US-075 criterion 2:
 * "resultados mostram as mudanças e os commits associados"). */
export function buildLineHistoryQuickPickItems(history: LineHistoryDto): QuickPickItemLike[] {
  if (history.entries.length === 0) {
    return [
      {
        id: NO_HISTORY_ITEM_ID,
        label: "No history for this line range",
        detail: `Lines ${history.range.start}-${history.range.end} as of ${history.revision.slice(0, 8)}`,
      },
    ];
  }
  return history.entries.map((entry) => {
    const item = commitQuickPickItem(entry.commit);
    const changedLines = entry.hunks.reduce((total, hunk) => total + hunk.lines.length, 0);
    return { ...item, detail: `${item.detail} • ${changedLines} line(s) touched in this range` };
  });
}
