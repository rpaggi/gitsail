import { describe, expect, it } from "vitest";

import { renderCommitDetailsText } from "../src/commitDetailsText";
import { CommitDto } from "../src/dto";

function commit(overrides: Partial<CommitDto> = {}): CommitDto {
  return {
    hash: "a".repeat(40),
    shortHash: "aaaaaaaa",
    parents: ["b".repeat(40)],
    author: { name: "Ada Lovelace", email: "ada@example.com" },
    committer: { name: "Ada Lovelace", email: "ada@example.com" },
    authorDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    subject: "Fix the off-by-one bug",
    body: "",
    decorations: [],
    isMerge: false,
    isRoot: false,
    ...overrides,
  };
}

describe("renderCommitDetailsText (T-206 criterion 2)", () => {
  it("includes hash, author, date, and subject verbatim (plain text, not Markdown)", () => {
    const text = renderCommitDetailsText(commit());
    expect(text).toContain(`commit ${"a".repeat(40)}`);
    expect(text).toContain("Ada Lovelace <ada@example.com>");
    expect(text).toContain("2023-11-14");
    expect(text).toContain("Fix the off-by-one bug");
  });

  it("shows the body when present", () => {
    const text = renderCommitDetailsText(commit({ body: "Longer explanation.\n\nSecond paragraph." }));
    expect(text).toContain("Longer explanation.");
    expect(text).toContain("Second paragraph.");
  });

  it("marks a root commit distinctly, without listing any parents", () => {
    const text = renderCommitDetailsText(commit({ parents: [], isRoot: true }));
    expect(text).toContain("root commit");
    expect(text).not.toMatch(/^parents:/m);
  });

  it("marks a merge commit and lists every parent", () => {
    const parents = ["b".repeat(40), "c".repeat(40)];
    const text = renderCommitDetailsText(commit({ parents, isMerge: true }));
    expect(text).toContain("merge commit");
    expect(text).toContain(parents.join(" "));
  });

  it("shows a distinct committer line only when it differs from the author", () => {
    const sameAuthor = renderCommitDetailsText(commit());
    expect(sameAuthor).not.toContain("Committer:");

    const differentCommitter = renderCommitDetailsText(
      commit({ committer: { name: "Grace Hopper", email: "grace@example.com" } }),
    );
    expect(differentCommitter).toContain("Committer: Grace Hopper <grace@example.com>");
  });

  it("never renders repository text as Markdown link/command syntax — it is plain text output", () => {
    const text = renderCommitDetailsText(
      commit({ subject: "[click](command:workbench.action.terminal.new)" }),
    );
    // Plain text is safe by construction (no Markdown renderer involved),
    // so the raw subject is expected here verbatim — this test documents
    // that guarantee rather than asserting any escaping happened.
    expect(text).toContain("[click](command:workbench.action.terminal.new)");
  });
});
