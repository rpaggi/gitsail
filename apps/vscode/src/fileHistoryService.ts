// File history browsing (T-207/US-074). Shapes the query and types the
// paginated result; the `git log` invocation and its parsing live in
// `git/gitClient.ts`.

import { GitClient } from "./git/gitClient";
import { GitResult, runGitQuery } from "./git/result";
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
  /** `false` stops history at rename boundaries; defaults to following
   * renames (US-074 criterion 3: a file's history survives its own
   * renames). */
  followRenames?: boolean;
}

export type FileHistoryPage = PageDto<CommitDto>;

export function getFileHistoryPage(
  client: GitClient,
  query: FileHistoryQuery,
): Promise<GitResult<FileHistoryPage>> {
  return runGitQuery(() =>
    client.getFileHistoryPage({
      repoRoot: query.repoRoot,
      filePath: query.filePath,
      revision: query.revision,
      cursor: query.cursor,
      limit: query.limit,
      followRenames: query.followRenames,
    }),
  );
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
