// Thin `GitSailCliClient` wrappers for single-commit queries (T-206/US-073,
// T-209/US-076). Every function here does exactly one `gitsail` subcommand
// call and returns its typed result via `CliResult` — no Git logic of any
// kind lives here, only argument assembly and result typing (US-069
// criterion 2, restated for EPIC-15: this extension never re-derives what
// the Core already computed, e.g. a merge commit's diff base).

import { GitSailCliClient } from "./cliClient";
import { CliResult, runCli } from "./cliResult";
import { CommitDiffDto, CommitDto, FileContentDto } from "./dto";

/** `gitsail commit <revision>` (US-073 criterion 2: hover's "open full
 * details" action re-queries the CLI rather than reusing a string already
 * parsed out of a decoration). */
export function getCommit(
  client: GitSailCliClient,
  repoRoot: string,
  revision: string,
): Promise<CliResult<CommitDto>> {
  return runCli<CommitDto>(client, ["commit", "--repo", repoRoot, revision]);
}

/** `gitsail commit-diff <revision>` (US-076 criterion 1). `data.base` is
 * `null` for a root commit and the resolved first-parent hash otherwise —
 * this extension only ever displays that policy, it never re-derives it
 * (see `dto.ts`'s `CommitDiffDto` doc comment). */
export function getCommitDiff(
  client: GitSailCliClient,
  repoRoot: string,
  revision: string,
): Promise<CliResult<CommitDiffDto>> {
  return runCli<CommitDiffDto>(client, ["commit-diff", "--repo", repoRoot, revision]);
}

/** `gitsail show-file <path> --revision <revision>` (US-076 criterion 3):
 * full content of one file as of one revision, for opening a historical
 * version read-only. `filePath` is relative to `repoRoot`. */
export function getFileContentAtRevision(
  client: GitSailCliClient,
  repoRoot: string,
  filePath: string,
  revision: string,
): Promise<CliResult<FileContentDto>> {
  return runCli<FileContentDto>(client, ["show-file", "--repo", repoRoot, filePath, "--revision", revision]);
}

/** Describes, for display, which base `gitsail commit-diff` used —
 * US-076 criterion 1 ("informa a política usada... sem fingir uma
 * comparação diferente da que foi feita"). Pure presentation text, derived
 * only from what the Core already reported (`commitDiff.base`), never a
 * re-derivation of the root/merge policy itself. */
export function describeCommitDiffBase(commitDiff: CommitDiffDto): string {
  return commitDiff.base === null
    ? "root commit — compared against the empty tree"
    : `compared against its first parent, ${commitDiff.base}`;
}
