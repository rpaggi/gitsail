// Real Git repositories in a temporary directory, for the suites that
// exercise `src/git/`.
//
// Deliberately a real `git`, never a mock or a fake binary. Before ADR-025
// this package tested its data access against a stub `gitsail` CLI
// (`test/fixtures/fake-cli.js`), which was defensible when the extension's
// job was to *relay* a JSON envelope someone else produced: a stub could
// reproduce that envelope exactly. It is not defensible now that the
// extension parses Git's own output, because a mock can only ever emit the
// output the author already believed Git emits — and the whole class of bug
// worth catching here is exactly the case where that belief is wrong (the
// blame porcelain repeating a header without its metadata block, a hunk
// range with no comma, a rename's `previousPath`). So these fixtures build
// real commits and let real `git` describe them.
//
// This mirrors what the Rust side already does in `gitsail-test-support`.

import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

/** Deterministic identity and dates, so an assertion can name an exact
 * author/timestamp instead of matching a pattern. */
const AUTHOR_NAME = "Blame Fixture";
const AUTHOR_EMAIL = "fixture@gitsail.test";
const FIXED_DATE = "1700000000 +0000";

export class TempRepo {
  private constructor(readonly root: string) {}

  static create(): TempRepo {
    const root = mkdtempSync(join(tmpdir(), "gitsail-vscode-"));
    const repo = new TempRepo(root);
    repo.git(["init", "-q", "-b", "main", "."]);
    repo.git(["config", "user.name", AUTHOR_NAME]);
    repo.git(["config", "user.email", AUTHOR_EMAIL]);
    // Keep the fixture independent of whatever the machine running the
    // suite has configured globally.
    repo.git(["config", "commit.gpgsign", "false"]);
    repo.git(["config", "core.autocrlf", "false"]);
    return repo;
  }

  git(args: string[]): string {
    return execFileSync("git", args, {
      cwd: this.root,
      stdio: "pipe",
      encoding: "utf8",
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: AUTHOR_NAME,
        GIT_AUTHOR_EMAIL: AUTHOR_EMAIL,
        GIT_COMMITTER_NAME: AUTHOR_NAME,
        GIT_COMMITTER_EMAIL: AUTHOR_EMAIL,
        GIT_AUTHOR_DATE: FIXED_DATE,
        GIT_COMMITTER_DATE: FIXED_DATE,
      },
    });
  }

  write(relativePath: string, contents: string): void {
    const absolute = join(this.root, relativePath);
    mkdirSync(dirname(absolute), { recursive: true });
    writeFileSync(absolute, contents, "utf8");
  }

  writeBytes(relativePath: string, contents: Buffer): void {
    const absolute = join(this.root, relativePath);
    mkdirSync(dirname(absolute), { recursive: true });
    writeFileSync(absolute, contents);
  }

  /** Stages everything and commits, returning the new commit's full hash. */
  commit(message: string): string {
    this.git(["add", "-A"]);
    this.git(["commit", "-q", "-m", message]);
    return this.head();
  }

  head(): string {
    return this.git(["rev-parse", "HEAD"]).trim();
  }

  dispose(): void {
    rmSync(this.root, { recursive: true, force: true });
  }
}

export const FIXTURE_AUTHOR = { name: AUTHOR_NAME, email: AUTHOR_EMAIL };
/** The epoch seconds in `FIXED_DATE`, for exact timestamp assertions. */
export const FIXTURE_EPOCH_SECONDS = 1700000000;
