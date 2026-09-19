// The test ADR-025 rests on.
//
// ADR-025 accepts one real cost: this extension now reimplements a
// read-only slice of Git reading in TypeScript, and that slice *can* drift
// from the Rust core — which is the very thing the "one core" architecture
// exists to prevent. The ADR's mitigation is that the slice is small and
// read-only. This suite is what makes that mitigation checkable rather
// than merely asserted: it builds the real `gitsail` CLI, points both it
// and `src/git/` at the *same* temporary repository, and requires them to
// produce the same DTOs.
//
// It replaces `coreParity.test.ts`, which compared this extension's
// *presentation* functions against real Core output. That comparison could
// only ever catch a formatting disagreement, because both sides were fed
// the same bytes from the same binary. The comparison below is strictly
// stronger: the two sides now derive their answers independently — Rust
// from `gitsail-git`'s parsers, TypeScript from `src/git/`'s — so a
// divergence in *either* one shows up here.
//
// A divergence failing this suite is the intended outcome. If it fails,
// the answer is to fix whichever side is wrong, not to loosen the
// assertion.

import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  BlameDto,
  CommitDiffDto,
  CommitDto,
  PageDto,
  RepositoryDto,
} from "../src/dto";
import { GitClient } from "../src/git/gitClient";
import { TempRepo } from "./support/tempRepo";

const WORKSPACE_ROOT = join(__dirname, "..", "..", "..");
const CLI_BINARY = join(
  WORKSPACE_ROOT,
  "target",
  "debug",
  process.platform === "win32" ? "gitsail.exe" : "gitsail",
);

// `docs/architecture/ci-policy.md` deliberately runs the `vscode` CI job
// with no Rust toolchain — it is a fast, Node-only job, independent from
// the `rust` matrix job that builds `gitsail-cli`. This suite needs a real,
// freshly built `gitsail-cli` to compare against (that is the whole point),
// so where `cargo` genuinely is not on `PATH` it skips itself rather than
// turning that Node-only job red for an environment reason unrelated to any
// extension defect. Anywhere a Rust toolchain *is* available (a full dev
// checkout, or a CI job that combines both), it runs for real and fails on
// a genuine divergence.
function cargoIsAvailable(): boolean {
  try {
    execFileSync("cargo", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

/** Runs the real `gitsail` CLI and returns the `data` of its JSON envelope.
 * The envelope shape is `gitsail-protocol`'s and is deliberately parsed
 * inline here rather than reintroducing `src/protocol.ts`: this is the one
 * place left that speaks it, and it speaks it as a *test harness* reading
 * another program's output, not as extension code. */
function cli<T>(repoRoot: string, args: string[]): T {
  const stdout = execFileSync(CLI_BINARY, [...args, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  const line = stdout
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
    .pop();
  if (line === undefined) {
    throw new Error("gitsail-cli printed no JSON output");
  }
  const envelope = JSON.parse(line) as
    | { status: "ok"; data: T }
    | { status: "error"; error: { code: string; message: string } };
  if (envelope.status !== "ok") {
    throw new Error(`gitsail-cli reported ${envelope.error.code}: ${envelope.error.message}`);
  }
  return envelope.data;
}

describe.skipIf(!cargoIsAvailable())(
  "src/git parity with the real gitsail-cli Core (ADR-025's drift mitigation)",
  () => {
    let repo: TempRepo;
    const client = new GitClient();
    let first: string;
    let second: string;
    let renamed: string;

    beforeAll(() => {
      // Builds the actual Core binary this repository ships — this suite
      // must compare against what Core *currently* returns, never a
      // previously built or hand-imagined binary.
      execFileSync("cargo", ["build", "--quiet", "-p", "gitsail-cli"], {
        cwd: WORKSPACE_ROOT,
        stdio: "inherit",
      });

      repo = TempRepo.create();
      repo.write("f.txt", "line1\nline2\nline3\n");
      first = repo.commit("initial import");

      repo.write("f.txt", "line1\nCHANGED-middle-line\nline3\n");
      repo.git(["add", "-A"]);
      repo.git([
        "commit",
        "-q",
        "-m",
        "change the middle line",
        "-m",
        "Explains why in the body.",
      ]);
      second = repo.head();

      repo.write("moved-later.txt", "a file with contents stable enough to detect a rename\n");
      repo.commit("add a file that will be renamed");
      repo.git(["mv", "moved-later.txt", "moved.txt"]);
      renamed = repo.commit("rename it");
    }, 180_000);

    afterAll(() => {
      repo?.dispose();
    });

    it("discovery agrees on root, bareness, HEAD state and branch", async () => {
      const core = cli<RepositoryDto>(repo.root, ["open", "--repo", repo.root]);
      const outcome = await client.discover(repo.root);

      expect(outcome.kind).toBe("repository");
      if (outcome.kind !== "repository") return;
      // Sanity: this is genuinely live Core output, not a stub.
      expect(core.currentBranch).toBe("main");
      expect(outcome.repository).toEqual(core);
    });

    it("a commit's every rendered field agrees, including the variable-length short hash", async () => {
      const core = cli<CommitDto>(repo.root, ["commit", "--repo", repo.root, second]);
      const mine = await client.getCommit(repo.root, second);

      expect(core.hash).toMatch(/^[0-9a-f]{40}$/);
      expect(core.subject).toBe("change the middle line");
      // `%h` is a variable-length abbreviation chosen by git, not a fixed
      // slice — a naive `hash.slice(0, 8)` would pass a hand-written
      // fixture and fail here, which is exactly the class of drift this
      // suite exists to catch.
      expect(mine.shortHash).toBe(core.shortHash);
      expect(mine).toEqual(core);
    });

    it("a root commit agrees on isRoot/parents", async () => {
      const core = cli<CommitDto>(repo.root, ["commit", "--repo", repo.root, first]);
      const mine = await client.getCommit(repo.root, first);

      expect(core.isRoot).toBe(true);
      expect(mine).toEqual(core);
    });

    it("blame agrees line for line, including authorship and origin", async () => {
      const core = cli<BlameDto>(repo.root, ["blame", "--repo", repo.root, "f.txt"]);
      const mine = await client.getBlame({ repoRoot: repo.root, filePath: "f.txt" });

      expect(core.lines).toHaveLength(3);
      expect(mine.lines).toEqual(core.lines);
      expect(mine.revision).toEqual(core.revision);
      expect(mine.file).toEqual(core.file);
    });

    it("blame of a working tree with an uncommitted line agrees on the local origin", async () => {
      repo.write("f.txt", "line1\nCHANGED-middle-line\nline3\nuncommitted\n");
      try {
        const core = cli<BlameDto>(repo.root, ["blame", "--repo", repo.root, "f.txt"]);
        const mine = await client.getBlame({ repoRoot: repo.root, filePath: "f.txt" });

        expect(core.lines.at(-1)?.origin).toBe("local");
        expect(mine.lines).toEqual(core.lines);
      } finally {
        repo.git(["checkout", "--", "f.txt"]);
      }
    });

    it("file history agrees on the commits and their order", async () => {
      const core = cli<PageDto<CommitDto>>(repo.root, [
        "log",
        "--repo",
        repo.root,
        "--path",
        "f.txt",
      ]);
      const mine = await client.getFileHistoryPage({ repoRoot: repo.root, filePath: "f.txt" });

      expect(core.items.map((c) => c.hash)).toEqual([second, first]);
      expect(mine.items).toEqual(core.items);
      expect(mine.hasMore).toBe(core.hasMore);
    });

    it("a commit diff agrees on the base, change type and every hunk line", async () => {
      const core = cli<CommitDiffDto>(repo.root, ["commit-diff", "--repo", repo.root, second]);
      const mine = await client.getCommitDiff(repo.root, second);

      expect(core.base).toBe(first);
      expect(mine).toEqual(core);
    });

    it("a root commit's diff agrees that the base is null", async () => {
      const core = cli<CommitDiffDto>(repo.root, ["commit-diff", "--repo", repo.root, first]);
      const mine = await client.getCommitDiff(repo.root, first);

      expect(core.base).toBeNull();
      expect(mine).toEqual(core);
    });

    it("a rename's diff agrees on changeType and previousPath", async () => {
      const core = cli<CommitDiffDto>(repo.root, ["commit-diff", "--repo", repo.root, renamed]);
      const mine = await client.getCommitDiff(repo.root, renamed);

      expect(core.diff.files[0].changeType).toBe("renamed");
      expect(core.diff.files[0].previousPath).toBe("moved-later.txt");
      expect(mine).toEqual(core);
    });
  },
);
