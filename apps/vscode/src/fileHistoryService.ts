// File history browsing (T-207/US-074). Wraps `gitsail log --path <file>`
// (US-018, already delivered) — this module never walks history itself,
// it only assembles the query and types the paginated result.

import { GitSailCliClient } from "./cliClient";
import { CliResult, runCli } from "./cliResult";
import { CommitDto, PageDto } from "./dto";

export interface FileHistoryQuery {
  repoRoot: string;
  /** Path relative to `repoRoot`. */
  filePath: string;
  /** The revision to start from (US-074 criterion 1: "preserva... a revisão
   * atual") — e.g. the buffer's current HEAD, or `undefined` for `HEAD`
   * itself. Always a plain revision expression, never re-resolved here. */
  revision?: string;
  cursor?: string;
  limit?: number;
  /** `false` stops history at rename boundaries (mirrors `gitsail log
   * --no-follow`); defaults to following renames, matching the CLI's own
   * default (US-074 criterion 3: a file's history survives its own
   * renames). */
  followRenames?: boolean;
}

export type FileHistoryPage = PageDto<CommitDto>;

export function getFileHistoryPage(
  client: GitSailCliClient,
  query: FileHistoryQuery,
): Promise<CliResult<FileHistoryPage>> {
  const args: string[] = ["log", "--repo", query.repoRoot];
  if (query.revision) {
    args.push(query.revision);
  }
  args.push("--path", query.filePath);
  if (query.limit !== undefined) {
    args.push("--limit", String(query.limit));
  }
  if (query.cursor !== undefined) {
    args.push("--cursor", query.cursor);
  }
  if (query.followRenames === false) {
    args.push("--no-follow");
  }
  return runCli<FileHistoryPage>(client, args);
}

export type FileHistoryOutcome =
  | { kind: "empty" }
  | { kind: "entries"; page: FileHistoryPage };

/** US-074 criterion 3: an empty result is never presented as an
 * undifferentiated blank list — a caller building UI from this must show an
 * explicit "no history" state instead of silently rendering nothing. */
export function describeFileHistoryOutcome(page: FileHistoryPage): FileHistoryOutcome {
  return page.items.length === 0 ? { kind: "empty" } : { kind: "entries", page };
}
