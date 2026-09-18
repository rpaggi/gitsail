// T-256/US-123 criterion 2: "VS Code presents hashes, authorship, and diffs
// coherent with the Core." Every other test in this suite exercises this
// extension's formatting functions against a *hand-written* fixture
// `CommitDto`/`BlameDto`/`CommitDiffDto` (see `commitDetailsText.test.ts`,
// `historyPresentation.test.ts`, `blameFormat.test.ts`) — a fixture can
// never catch a divergence between what this extension *assumes* a DTO
// field means and what `gitsail-cli` (the actual Core binary every user
// runs) actually puts in it.
//
// This suite closes that gap: it builds the real `gitsail` binary, runs it
// against a real, temporary Git repository through the exact same
// `commitService.ts`/`blameService.ts`/`fileHistoryService.ts` wrappers
// `historyController.ts` itself calls in production, and only then feeds
// the resulting *real* DTOs into this extension's own presentation
// functions (`commitDetailsText.ts`, `blameFormat.ts`,
// `historyPresentation.ts`, `historyController.ts`'s
// `buildCommitDiffFileQuickPickItems`) — proving there is no divergence
// (US-123 criterion 2's own example: "hash truncado errado, autor
// formatado diferente do que o Core define").

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  abbreviateHash,
  DEFAULT_BLAME_DATE_STYLE,
  DEFAULT_BLAME_DELAY_MS,
  DEFAULT_BLAME_FORMAT,
  DEFAULT_BLAME_MODE,
  describeBlameLine,
  formatGitTimestamp,
} from "../src/blameFormat";
import { getBlame } from "../src/blameService";
import { GitSailCliClient } from "../src/cliClient";
import { renderCommitDetailsText } from "../src/commitDetailsText";
import { describeCommitDiffBase, getCommit, getCommitDiff } from "../src/commitService";
import { describeFileHistoryOutcome, getFileHistoryPage } from "../src/fileHistoryService";
import { buildCommitDiffFileQuickPickItems } from "../src/historyController";
import { buildFileHistoryQuickPickItems } from "../src/historyPresentation";

const WORKSPACE_ROOT = join(__dirname, "..", "..", "..");
const CLI_BINARY = join(WORKSPACE_ROOT, "target", "debug", process.platform === "win32" ? "gitsail.exe" : "gitsail");
const FULL_HASH = /^[0-9a-f]{40}$/;

function git(cwd: string, args: string[]): void {
  execFileSync("git", args, { cwd, stdio: "pipe" });
}

// `docs/architecture/ci-policy.md` deliberately runs the `vscode` CI job
// (`npm ci`, `npx vitest run`, `npx tsc -p ./`) with no Rust toolchain — it
// is a fast, Node-only job, independent from the `rust` matrix job that
// actually builds `gitsail-cli`. This suite needs a real, freshly built
// `gitsail-cli` to compare against (that is the whole point — see the
// module doc above), so where `cargo` genuinely is not on `PATH` it skips
// itself instead of turning that Node-only job red for an environment
// reason unrelated to any extension defect. Anywhere a Rust toolchain *is*
// available (a full dev checkout, or a future CI job that combines both —
// see this task's own regression report for that recommendation), it runs
// for real and fails on a genuine divergence.
function cargoIsAvailable(): boolean {
  try {
    execFileSync("cargo", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

describe.skipIf(!cargoIsAvailable())("VS Code presentation vs. the real gitsail-cli Core (T-256/US-123 criterion 2)", () => {
  let repoRoot: string;
  let client: GitSailCliClient;

  beforeAll(() => {
    // Builds the actual Core binary this repository ships — this suite
    // must compare against what Core *currently* returns, never a
    // previously built or hand-imagined binary.
    execFileSync("cargo", ["build", "--quiet", "-p", "gitsail-cli"], {
      cwd: WORKSPACE_ROOT,
      stdio: "inherit",
    });

    repoRoot = mkdtempSync(join(tmpdir(), "gitsail-core-parity-"));
    git(repoRoot, ["init", "--quiet", "--initial-branch=main"]);
    git(repoRoot, ["config", "user.name", "Core Parity Author"]);
    git(repoRoot, ["config", "user.email", "core-parity@gitsail.test"]);

    writeFileSync(join(repoRoot, "f.txt"), "line1\nline2\nline3\n");
    git(repoRoot, ["add", "-A"]);
    git(repoRoot, ["commit", "--quiet", "-m", "initial import"]);

    writeFileSync(join(repoRoot, "f.txt"), "line1\nCHANGED-middle-line\nline3\n");
    git(repoRoot, ["add", "-A"]);
    git(repoRoot, ["commit", "--quiet", "-m", "change the middle line\n\nExplains why in the body."]);

    client = new GitSailCliClient({ binaryPath: CLI_BINARY });
  }, 180_000);

  afterAll(() => {
    rmSync(repoRoot, { recursive: true, force: true });
  });

  it("renderCommitDetailsText embeds Core's exact hash/author/subject for HEAD, never a truncated or re-derived value", async () => {
    const result = await getCommit(client, repoRoot, "HEAD");
    expect(result.kind).toBe("ok");
    if (result.kind !== "ok") {
      return;
    }
    const commit = result.value;

    // Sanity: this is genuinely live Core output, not a stub.
    expect(commit.hash).toMatch(FULL_HASH);
    expect(commit.author.name).toBe("Core Parity Author");
    expect(commit.author.email).toBe("core-parity@gitsail.test");
    expect(commit.subject).toBe("change the middle line");
    expect(commit.body.trim()).toBe("Explains why in the body.");
    // Core's own short hash (`%h`) is a variable-length abbreviation, not a
    // fixed-length slice — asserting it differs from a naive 8-char
    // truncation whenever they happen to diverge would be flaky the other
    // way, so this only pins that Core actually reports one, distinctly
    // from the full hash.
    expect(commit.hash.startsWith(commit.shortHash)).toBe(true);

    const text = renderCommitDetailsText(commit);
    const lines = text.split("\n");

    // The full 40-character hash must appear verbatim, on its own line —
    // the extension must never substitute Core's own `shortHash` or a
    // home-grown truncation for the "commit <hash>" line real `git show`/
    // `gitsail commit` use. `shortHash` is a genuine prefix of `hash`, so
    // this is asserted as an exact line match, never a substring check (a
    // substring check could never fail here, since "commit <hash>"
    // trivially contains "commit <any prefix of hash>").
    expect(lines).toContain(`commit ${commit.hash}`);
    expect(lines).not.toContain(`commit ${commit.shortHash}`);
    // Author formatted exactly as Core's own CLI (`gitsail-cli`'s
    // `render_commit_block`: "Author: {name} <{email}>") defines it — same
    // field order, same angle-bracket wrapping.
    expect(text).toContain(`Author:    ${commit.author.name} <${commit.author.email}>`);
    expect(text).toContain(`AuthorDate: ${formatGitTimestamp(commit.authorDate)}`);
    expect(text).toContain(commit.subject);
    expect(text).toContain(commit.body);
  });

  it("blame hover/decoration reports the exact hash/author Core's blame engine attributes each line to", async () => {
    const blameResult = await getBlame(client, { repoRoot, filePath: "f.txt" });
    expect(blameResult.kind).toBe("ok");
    if (blameResult.kind !== "ok") {
      return;
    }
    const blame = blameResult.value;
    expect(blame.lines).toHaveLength(3);

    const changedLine = blame.lines[1];
    expect(changedLine.origin).toBe("committed");
    expect(changedLine.content).toBe("CHANGED-middle-line");
    expect(changedLine.commit).toMatch(FULL_HASH);
    expect(changedLine.author.name).toBe("Core Parity Author");

    // Cross-check: independently ask Core for the commit that hash names
    // (a second, separate CLI call) and confirm it is the same commit the
    // blame line's own author/hash claimed — never presumed consistent.
    const commitResult = await getCommit(client, repoRoot, changedLine.commit);
    expect(commitResult.kind).toBe("ok");
    if (commitResult.kind !== "ok") {
      return;
    }
    const commit = commitResult.value;
    expect(commit.subject).toBe("change the middle line");
    expect(commit.author.name).toBe(changedLine.author.name);
    expect(commit.author.email).toBe(changedLine.author.email);

    const config = {
      enabled: true,
      format: DEFAULT_BLAME_FORMAT,
      mode: DEFAULT_BLAME_MODE,
      delayMs: DEFAULT_BLAME_DELAY_MS,
      dateStyle: DEFAULT_BLAME_DATE_STYLE,
    };
    const described = describeBlameLine(changedLine, config, commit.subject, false);

    // The hover's author line and hash line must be Core's own values
    // verbatim, and the inline decoration's `${shortHash}` must be a real
    // prefix of the exact hash Core's blame engine reported for this line
    // (never a different/unrelated hash — the "hash truncado errado"
    // hazard US-123 criterion 2 names explicitly).
    expect(described.hoverLines).toContain(`${changedLine.author.name} <${changedLine.author.email}>`);
    expect(described.hoverLines.some((line) => line.includes(changedLine.commit))).toBe(true);
    expect(described.contentText).toContain(abbreviateHash(changedLine.commit));
    expect(changedLine.commit.startsWith(abbreviateHash(changedLine.commit))).toBe(true);
    expect(described.contentText).toContain(commit.subject);
  });

  it("the commit-diff file picker and base description match Core's own resolved base/changed files for HEAD", async () => {
    const headResult = await getCommit(client, repoRoot, "HEAD");
    const diffResult = await getCommitDiff(client, repoRoot, "HEAD");
    expect(headResult.kind).toBe("ok");
    expect(diffResult.kind).toBe("ok");
    if (headResult.kind !== "ok" || diffResult.kind !== "ok") {
      return;
    }
    const head = headResult.value;
    const commitDiff = diffResult.value;

    // Cross-check: `commit-diff`'s reported base is the exact same
    // first-parent hash `commit HEAD` itself reports — two separate Core
    // calls agreeing on the same fact, never assumed.
    expect(head.isRoot).toBe(false);
    expect(commitDiff.base).toBe(head.parents[0]);

    const description = describeCommitDiffBase(commitDiff);
    expect(description).toContain(commitDiff.base as string);

    expect(commitDiff.diff.files.map((file) => file.path)).toContain("f.txt");
    const items = buildCommitDiffFileQuickPickItems(commitDiff);
    const fileItem = items.find((item) => item.id === "f.txt");
    expect(fileItem).toBeDefined();
    expect(fileItem?.label).toBe("f.txt");
    expect(fileItem?.description).toBe(commitDiff.diff.files.find((file) => file.path === "f.txt")?.changeType);
  });

  it("the file-history quick pick shows Core's own short hash/author/date for each real commit, unmodified", async () => {
    const pageResult = await getFileHistoryPage(client, { repoRoot, filePath: "f.txt" });
    expect(pageResult.kind).toBe("ok");
    if (pageResult.kind !== "ok") {
      return;
    }
    const page = pageResult.value;
    expect(page.items).toHaveLength(2);

    const outcome = describeFileHistoryOutcome(page);
    const items = buildFileHistoryQuickPickItems(outcome);
    expect(items).toHaveLength(2);

    const [latest] = page.items;
    const [latestItem] = items;
    expect(latestItem.id).toBe(latest.hash);
    expect(latestItem.label).toBe(`${latest.shortHash}  ${latest.subject}`);
    expect(latestItem.description).toBe(latest.author.name);
    expect(latestItem.detail).toBe(formatGitTimestamp(latest.authorDate));
  });
});
