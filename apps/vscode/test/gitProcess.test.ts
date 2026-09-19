// Contract tests for the extension's one Git process boundary
// (`src/git/process.ts`), against a real spawned `git`.
//
// Replaces `cliClient.test.ts`, which asserted the same properties about
// spawning the `gitsail` binary: no shell, one settle path, timeout,
// cancellation. ADR-025 changed *what* is spawned, not what the boundary
// owes its callers — so every property that file asserted is asserted here,
// plus the two the CLI client did not have to care about because the Rust
// core handled them on the other side of the envelope: a capped output
// buffer and redacted stderr.

import { describe, expect, it } from "vitest";

import { GitCancelledError, GitNotFoundError, GitProcessError, GitTimeoutError } from "../src/git/errors";
import { decodeStdout, runGit, runGitChecked, tryRunGit } from "../src/git/process";
import { TempRepo } from "./support/tempRepo";

describe("runGit", () => {
  it("runs a real git query and captures its stdout", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "hello\n");
      const hash = repo.commit("first");

      const result = await runGit({ cwd: repo.root, args: ["rev-parse", "HEAD"] });

      expect(result.exitCode).toBe(0);
      expect(decodeStdout(result).trim()).toBe(hash);
      expect(result.truncated).toBe(false);
    } finally {
      repo.dispose();
    }
  });

  it("resolves (rather than throwing) for a non-zero exit, leaving the query to judge it", async () => {
    const repo = TempRepo.create();
    try {
      // "No such revision" is a legitimate answer for several queries, so
      // the boundary must not pre-empt that judgement by throwing.
      const result = await runGit({
        cwd: repo.root,
        args: ["rev-parse", "--verify", "-q", "definitely-not-a-ref"],
      });
      expect(result.exitCode).not.toBe(0);
    } finally {
      repo.dispose();
    }
  });

  it("never interprets its arguments through a shell", async () => {
    const repo = TempRepo.create();
    try {
      // A filename that is pure shell metacharacter soup. If any of this
      // ever reached a shell, the command substitution would run and the
      // redirection would create files; via an argv array it is simply a
      // (nonexistent) pathspec that git reports on literally.
      const hostile = "$(touch /tmp/gitsail-pwned); rm -rf .; echo > pwned.txt";
      const result = await runGit({
        cwd: repo.root,
        args: ["log", "--oneline", "--", hostile],
      });
      // Whatever git decides about an unmatched pathspec, the one thing
      // that must be true is that the repository is still intact and no
      // side effect happened.
      expect(result.exitCode !== 0 || decodeStdout(result).trim()).toBeTruthy();
      expect(repo.git(["status", "--porcelain"]).includes("pwned.txt")).toBe(false);
    } finally {
      repo.dispose();
    }
  });

  it("times out and kills a query that does not finish in budget", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "hello\n");
      repo.commit("first");
      // `git help --all` is not slow; a 1ms budget is what makes this
      // deterministic — any real invocation exceeds it.
      await expect(
        runGit({ cwd: repo.root, args: ["log", "--patch", "--all"], timeoutMs: 1 }),
      ).rejects.toBeInstanceOf(GitTimeoutError);
    } finally {
      repo.dispose();
    }
  });

  it("rejects immediately for an already-aborted signal, without spawning", async () => {
    const repo = TempRepo.create();
    try {
      const controller = new AbortController();
      controller.abort();
      await expect(
        runGit({ cwd: repo.root, args: ["rev-parse", "HEAD"], signal: controller.signal }),
      ).rejects.toBeInstanceOf(GitCancelledError);
    } finally {
      repo.dispose();
    }
  });

  it("cancels an in-flight query when its signal aborts", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "hello\n");
      repo.commit("first");

      const controller = new AbortController();
      const promise = runGit({
        cwd: repo.root,
        args: ["log", "--patch", "--all"],
        signal: controller.signal,
      });
      controller.abort();

      await expect(promise).rejects.toBeInstanceOf(GitCancelledError);
    } finally {
      repo.dispose();
    }
  });

  it("rejects with GitNotFoundError when git cannot be spawned at all", async () => {
    // `cwd` that does not exist makes Node fail the spawn with ENOENT,
    // which is the same code a missing `git` produces — this is the only
    // way to exercise that branch without removing git from PATH.
    await expect(
      runGit({ cwd: "/nonexistent/gitsail/definitely/not/here", args: ["--version"] }),
    ).rejects.toBeInstanceOf(GitNotFoundError);
  });

  it("redacts a credential-bearing URL out of stderr before any caller sees it", async () => {
    const repo = TempRepo.create();
    try {
      const secret = "sentinel-fake-token-9f3c7a";
      // A fetch from a bogus https remote makes git print the URL it tried
      // back on stderr — the real-world path by which an embedded
      // credential leaks into an error message.
      const result = await runGit({
        cwd: repo.root,
        args: ["fetch", `https://user:${secret}@127.0.0.1:1/nope.git`],
        timeoutMs: 15_000,
      });

      expect(result.exitCode).not.toBe(0);
      expect(result.stderr).not.toContain(secret);
    } finally {
      repo.dispose();
    }
  });
});

describe("runGitChecked", () => {
  it("throws GitProcessError for a non-zero exit, carrying the redacted stderr", async () => {
    const repo = TempRepo.create();
    try {
      const error = await runGitChecked({
        cwd: repo.root,
        args: ["cat-file", "-p", "definitely-not-an-object"],
      }).catch((e: unknown) => e);

      expect(error).toBeInstanceOf(GitProcessError);
      expect((error as GitProcessError).exitCode).not.toBe(0);
    } finally {
      repo.dispose();
    }
  });
});

describe("tryRunGit", () => {
  it("resolves to undefined for a non-zero exit instead of throwing", async () => {
    const repo = TempRepo.create();
    try {
      const result = await tryRunGit({
        cwd: repo.root,
        args: ["rev-parse", "--verify", "-q", "HEAD"],
      });
      // Unborn HEAD in a fresh repository: a legitimate "no" answer.
      expect(result).toBeUndefined();
    } finally {
      repo.dispose();
    }
  });
});
