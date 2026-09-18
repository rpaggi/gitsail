// Line/range history (T-208/US-075). Wraps `gitsail line-history` (US-019,
// already delivered by the Core's `GetLineHistory`/`git log -L` engine,
// including correctly tracking a range across renames/line shifts *within
// committed history* — this module never re-derives that tracking, it only
// assembles the query from the editor's current selection).

import { GitSailCliClient } from "./cliClient";
import { CliResult, runCli } from "./cliResult";
import { LineHistoryDto } from "./dto";

export interface LineRangeSelection {
  /** 1-based, inclusive — the same convention `gitsail`'s `--range
   * START-END` and `BlameRequest`/`LineHistoryRequest` already use, so no
   * off-by-one translation happens anywhere except right at the VS Code
   * boundary (0-based `Position`/`Selection`) in `extension.ts`. */
  startLine: number;
  endLine: number;
}

export interface LineHistoryQuery {
  repoRoot: string;
  /** Path relative to `repoRoot`. */
  filePath: string;
  range: LineRangeSelection;
  /** The revision to trace from (US-075 criterion 1: "HEAD ou a revisão do
   * buffer atual"). `undefined` lets the CLI default to `HEAD` itself
   * (`GetLineHistory`'s own documented default) — this module never
   * resolves "the buffer's current revision" itself; in this tool's model
   * an open buffer's revision *is* the repository's checked-out HEAD, which
   * is exactly what omitting `--revision` already asks the Core for. */
  revision?: string;
}

export function getLineHistory(
  client: GitSailCliClient,
  query: LineHistoryQuery,
): Promise<CliResult<LineHistoryDto>> {
  const args: string[] = [
    "line-history",
    "--repo",
    query.repoRoot,
    query.filePath,
    "--range",
    `${query.range.startLine}-${query.range.endLine}`,
  ];
  if (query.revision) {
    args.push("--revision", query.revision);
  }
  return runCli<LineHistoryDto>(client, args);
}

/** US-075 criterion 3: an editor buffer with unsaved changes may have
 * inserted/removed lines above the queried range, so the range numbers sent
 * to `gitsail line-history` (always the *last-saved-to-disk* line numbers,
 * since that is all the Core can see) can silently disagree with what is
 * currently selected on screen. This never invents a corrected range or a
 * fabricated attribution for the mismatch — it only surfaces the fact, the
 * same "disk vs. buffer" disclaimer convention as `blameFormat.ts`. */
export function describeLineHistoryBufferCaveat(documentDirty: boolean): string | undefined {
  if (!documentDirty) {
    return undefined;
  }
  return "This file has unsaved editor changes; the queried line range refers to the version saved on disk, which may no longer match the current selection's line numbers.";
}
