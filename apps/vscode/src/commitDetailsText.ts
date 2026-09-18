// Renders a `CommitDto` as a plain-text document (T-206 criterion 2's
// "detalhes completos" panel, backed by `gitsail-commit:` — see
// `commitDetailsUri.ts`). Deliberately *plain text*, not Markdown: this
// becomes a real editor document's content, not a rendered hover, so there
// is no Markdown-injection surface here in the first place — repository
// text (subject/body/author) is shown verbatim, exactly as a `git show`
// text-mode viewer would, safe by construction because plain text editor
// buffers do not execute anything.

import { formatGitTimestamp } from "./blameFormat";
import { CommitDto } from "./dto";

export function renderCommitDetailsText(commit: CommitDto): string {
  const lines: string[] = [];
  lines.push(`commit ${commit.hash}`);
  if (commit.parents.length > 0) {
    lines.push(`parents: ${commit.parents.join(" ")}`);
  }
  if (commit.isRoot) {
    lines.push("(root commit — no parents)");
  }
  if (commit.isMerge) {
    lines.push("(merge commit)");
  }
  lines.push(`Author:    ${commit.author.name} <${commit.author.email}>`);
  lines.push(`AuthorDate: ${formatGitTimestamp(commit.authorDate)}`);
  if (commit.committer.name !== commit.author.name || commit.committer.email !== commit.author.email) {
    lines.push(`Committer: ${commit.committer.name} <${commit.committer.email}>`);
    lines.push(`CommitDate: ${formatGitTimestamp(commit.commitDate)}`);
  }
  lines.push("");
  lines.push(commit.subject);
  if (commit.body.trim().length > 0) {
    lines.push("");
    lines.push(commit.body);
  }
  return `${lines.join("\n")}\n`;
}
