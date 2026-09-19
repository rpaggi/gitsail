// Commit/diff/file-content service wrappers, against real repositories.
//
// The pre-ADR-025 version of this suite asserted the argument list sent to
// `gitsail` (`["commit", "--repo", ...]`). Those command lines no longer
// exist, so each of those tests is restated as the behavior it was standing
// in for: does this wrapper return the right commit, the right diff base,
// the right file content. `describeCommitDiffBase` is pure presentation and
// its tests are unchanged.

import { describe, expect, it } from "vitest";

import {
  describeCommitDiffBase,
  getCommit,
  getCommitDiff,
  getFileContentAtRevision,
} from "../src/commitService";
import { CommitDiffDto } from "../src/dto";
import { GitClient } from "../src/git/gitClient";
import { TempRepo } from "./support/tempRepo";

const client = new GitClient();

describe("getCommit", () => {
  it("returns the requested commit, resolving a relative revision expression", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "one\n");
      const first = repo.commit("first");
      repo.write("a.txt", "two\n");
      const second = repo.commit("second");

      expect((await getCommit(client, repo.root, second)).kind).toBe("ok");

      const parent = await getCommit(client, repo.root, "HEAD~1");
      expect(parent.kind).toBe("ok");
      if (parent.kind !== "ok") return;
      expect(parent.value.hash).toBe(first);
      expect(parent.value.subject).toBe("first");
    } finally {
      repo.dispose();
    }
  });

  it("returns an error result for an unresolvable revision instead of a fabricated commit", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "one\n");
      repo.commit("first");
      const result = await getCommit(client, repo.root, "no-such-ref");
      expect(result.kind).toBe("error");
    } finally {
      repo.dispose();
    }
  });
});

describe("getCommitDiff", () => {
  it("returns the commit's diff against the base the policy selected", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "one\n");
      const first = repo.commit("first");
      repo.write("a.txt", "two\n");
      const second = repo.commit("second");

      const result = await getCommitDiff(client, repo.root, second);

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.target).toBe(second);
      expect(result.value.base).toBe(first);
      expect(result.value.diff.files.map((f) => f.path)).toEqual(["a.txt"]);
    } finally {
      repo.dispose();
    }
  });
});

describe("getFileContentAtRevision", () => {
  it("returns the file's content as it was at that revision", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("src/a.txt", "original\n");
      const first = repo.commit("first");
      repo.write("src/a.txt", "changed\n");
      repo.commit("second");

      const result = await getFileContentAtRevision(client, repo.root, "src/a.txt", first);

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.kind).toBe("text");
      if (result.value.kind !== "text") return;
      expect(result.value.content).toBe("original\n");
    } finally {
      repo.dispose();
    }
  });

  it("reports a missing file as a value, never as a failure", async () => {
    const repo = TempRepo.create();
    try {
      repo.write("a.txt", "one\n");
      const first = repo.commit("first");

      const result = await getFileContentAtRevision(client, repo.root, "nope.txt", first);

      expect(result.kind).toBe("ok");
      if (result.kind !== "ok") return;
      expect(result.value.kind).toBe("missing");
    } finally {
      repo.dispose();
    }
  });
});

describe("describeCommitDiffBase (US-076 criterion 1)", () => {
  it("names the empty tree for a root commit", () => {
    const commitDiff: CommitDiffDto = { target: "a".repeat(40), base: null, diff: { files: [] } };
    expect(describeCommitDiffBase(commitDiff)).toMatch(/root commit/i);
    expect(describeCommitDiffBase(commitDiff)).toMatch(/empty tree/i);
  });

  it("names the exact first-parent hash for a non-root commit, never a vaguer description", () => {
    const base = "b".repeat(40);
    const commitDiff: CommitDiffDto = { target: "a".repeat(40), base, diff: { files: [] } };
    expect(describeCommitDiffBase(commitDiff)).toContain(base);
    expect(describeCommitDiffBase(commitDiff)).toMatch(/first parent/i);
  });
});
