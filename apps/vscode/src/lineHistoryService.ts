// Line/range history (T-208/US-075). Shapes the query from the editor's
// current selection; the `git log -L` invocation and its parsing live in
// `git/gitClient.ts`, which is also what tracks a range across renames and
// line shifts *within committed history* — this module never re-derives
// that tracking.

import { GitClient } from "./git/gitClient";
import { GitResult, runGitQuery } from "./git/result";
import { LineHistoryDto } from "./dto";

export interface LineRangeSelection {
  /** 1-based, inclusive — the same convention `LineHistoryRequest` already
   * uses, so no off-by-one translation happens anywhere except right at the
   * VS Code boundary (0-based `Position`/`Selection`) in `extension.ts`. */
  startLine: number;
  endLine: number;
}

export interface LineHistoryQuery {
  repoRoot: string;
  /** Path relative to `repoRoot`. */
  filePath: string;
  range: LineRangeSelection;
  /** The revision to trace from (US-075 criterion 1: "HEAD ou a revisão do
   * buffer atual"). `undefined` defaults to `HEAD` — this module never
   * resolves "the buffer's current revision" itself; in this tool's model
   * an open buffer's revision *is* the repository's checked-out HEAD. */
  revision?: string;
}

export function getLineHistory(
  client: GitClient,
  query: LineHistoryQuery,
): Promise<GitResult<LineHistoryDto>> {
  return runGitQuery(() =>
    client.getLineHistory({
      repoRoot: query.repoRoot,
      filePath: query.filePath,
      startLine: query.range.startLine,
      endLine: query.range.endLine,
      revision: query.revision,
    }),
  );
}

/** US-075 criterion 3: an editor buffer with unsaved changes may have
 * inserted/removed lines above the queried range, so the range numbers sent
 * to `git log -L` (always the *last-saved-to-disk* line numbers, since that
 * is all Git can see) can silently disagree with what is currently selected
 * on screen. This never invents a corrected range or a fabricated
 * attribution for the mismatch — it only surfaces the fact, the same
 * "disk vs. buffer" disclaimer convention as `blameFormat.ts`. */
export function describeLineHistoryBufferCaveat(documentDirty: boolean): string | undefined {
  if (!documentDirty) {
    return undefined;
  }
  return "This file has unsaved editor changes; the queried line range refers to the version saved on disk, which may no longer match the current selection's line numbers.";
}
