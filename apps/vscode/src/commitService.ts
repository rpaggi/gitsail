// Thin `GitClient` wrappers for single-commit queries (T-206/US-073,
// T-209/US-076). Every function here is one query returning its typed
// result via `GitResult` — the Git reading itself lives in `git/`, never
// here.

import { GitClient } from "./git/gitClient";
import { GitResult, runGitQuery } from "./git/result";
import { CommitDiffDto, CommitDto, FileContentDto } from "./dto";

/** One commit's metadata (US-073 criterion 2: the hover's "open full
 * details" action re-queries rather than reusing a string already parsed
 * out of a decoration). */
export function getCommit(
  client: GitClient,
  repoRoot: string,
  revision: string,
): Promise<GitResult<CommitDto>> {
  return runGitQuery(() => client.getCommit(repoRoot, revision));
}

/** One commit's diff against its resolved base (US-076 criterion 1).
 * `base` is `null` for a root commit and the first-parent hash otherwise;
 * that policy lives in `git/gitClient.ts`, and this extension's UI only
 * ever displays what it decided (see `dto.ts`'s `CommitDiffDto` doc). */
export function getCommitDiff(
  client: GitClient,
  repoRoot: string,
  revision: string,
): Promise<GitResult<CommitDiffDto>> {
  return runGitQuery(() => client.getCommitDiff(repoRoot, revision));
}

/** Full content of one file as of one revision (US-076 criterion 3), for
 * opening a historical version read-only. `filePath` is relative to
 * `repoRoot`. `binary`/`missing` come back as ordinary values, never
 * errors. */
export function getFileContentAtRevision(
  client: GitClient,
  repoRoot: string,
  filePath: string,
  revision: string,
): Promise<GitResult<FileContentDto>> {
  return runGitQuery(() => client.getFileContentAtRevision(repoRoot, filePath, revision));
}

/** Describes, for display, which base the commit diff used — US-076
 * criterion 1 ("informa a política usada... sem fingir uma comparação
 * diferente da que foi feita"). Pure presentation text, derived only from
 * `commitDiff.base`, never a re-derivation of the root/merge policy. */
export function describeCommitDiffBase(commitDiff: CommitDiffDto): string {
  return commitDiff.base === null
    ? "root commit — compared against the empty tree"
    : `compared against its first parent, ${commitDiff.base}`;
}
