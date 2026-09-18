import { describe, expect, it } from "vitest";

import { HoverContentBuilder, escapeMarkdownText } from "../src/hoverSanitizer";

describe("escapeMarkdownText (T-206 criterion 3)", () => {
  it("neutralizes a command: link disguised as a commit message", () => {
    const malicious = "[Click here for details](command:workbench.action.terminal.new)";
    const escaped = escapeMarkdownText(malicious);
    // The brackets/parens that make this a Markdown link are escaped, so it
    // can only ever render as literal text — the exact link syntax
    // `](command:` must never survive unescaped.
    expect(escaped).not.toMatch(/]\(command:/);
    expect(escaped).toContain("\\[Click here for details\\]");
    expect(escaped).toContain("\\(command:workbench");
    expect(escaped).toContain("\\)");
  });

  it("neutralizes raw HTML/script-like markup", () => {
    const malicious = "<script>alert(1)</script>";
    const escaped = escapeMarkdownText(malicious);
    expect(escaped).toBe("\\<script\\>alert\\(1\\)\\</script\\>");
  });

  it("leaves text with no Markdown-special characters untouched", () => {
    const normal = "Fix the pagination cursor";
    expect(escapeMarkdownText(normal)).toBe(normal);
  });

  it("escapes ordinary punctuation too (over-escaping is the safe direction)", () => {
    // A period or hyphen in ordinary prose is still Markdown syntax in the
    // right position (e.g. a numbered/bulleted list); escaping it is a
    // harmless cosmetic cost, not a correctness bug, and this module always
    // prefers that over under-escaping something that turns out dangerous.
    expect(escapeMarkdownText("Fix the off-by-one error.")).toBe(
      "Fix the off\\-by\\-one error\\.",
    );
  });

  it("escapes every documented Markdown control character", () => {
    const input = "\\`*_{}[]()#+-.!|<>~^";
    const escaped = escapeMarkdownText(input);
    for (const char of input) {
      expect(escaped).toContain(`\\${char}`);
    }
  });
});

describe("HoverContentBuilder", () => {
  it("escapes only untrusted lines, leaving trusted (GitSail-authored) lines verbatim", () => {
    const content = new HoverContentBuilder()
      .addTrustedLine("**Commit details**")
      .addUntrustedLine("[evil](command:danger)")
      .build();

    expect(content.markdown).toContain("**Commit details**"); // verbatim, still bold
    expect(content.markdown).toContain("\\[evil\\]\\(command:danger\\)");
  });

  it("joins lines with a Markdown hard line break", () => {
    const content = new HoverContentBuilder().addTrustedLine("one").addTrustedLine("two").build();
    expect(content.markdown).toBe("one  \ntwo");
  });

  it("supports blank lines as paragraph breaks", () => {
    const content = new HoverContentBuilder().addTrustedLine("one").addBlankLine().addTrustedLine("two").build();
    expect(content.markdown).toBe("one  \n  \ntwo");
  });

  it("a realistic malicious commit subject/body never survives as an active Markdown link or raw tag", () => {
    const content = new HoverContentBuilder()
      .addTrustedLine("Commit abc1234")
      .addUntrustedLine("Please [review this](command:workbench.action.files.openFile) urgently")
      .addUntrustedLine("<a href=\"javascript:alert(1)\">click</a>")
      .build();

    // The Markdown link syntax `](command:` requires the closing `]` and
    // opening `(` to be adjacent; escaping inserts a backslash between them,
    // so that exact adjacency never survives.
    expect(content.markdown).not.toMatch(/\]\(command:/);
    // Every `<` is individually backslash-escaped, so it can only ever
    // render as a literal angle bracket, never open an HTML tag.
    expect(content.markdown).toContain("\\<a href=");
    expect(content.markdown).toContain("\\</a\\>");
  });
});

describe("HoverContentBuilder.addCommandLink (T-206 criteria 2 and 3 reconciled)", () => {
  it("renders a real Markdown command link with JSON-encoded arguments", () => {
    const content = new HoverContentBuilder()
      .addCommandLink("Open full commit details", "gitsail.openCommitDetails", { hash: "abc123" })
      .build();

    expect(content.markdown).toBe(
      `[Open full commit details](command:gitsail.openCommitDetails?${encodeURIComponent(
        JSON.stringify({ hash: "abc123" }),
      )})`,
    );
  });

  it("collects every distinct command id used into enabledCommands, for a scoped isTrusted", () => {
    const content = new HoverContentBuilder()
      .addCommandLink("Open details", "gitsail.openCommitDetails", { hash: "abc" })
      .addCommandLink("Copy hash", "gitsail.copyCommitHash", { hash: "abc" })
      .addCommandLink("Open details again", "gitsail.openCommitDetails", { hash: "def" })
      .build();

    expect(content.enabledCommands.sort()).toEqual(["gitsail.copyCommitHash", "gitsail.openCommitDetails"]);
  });

  it("plain trusted/untrusted content never contributes any enabled command", () => {
    const content = new HoverContentBuilder().addTrustedLine("x").addUntrustedLine("[y](command:z)").build();
    expect(content.enabledCommands).toEqual([]);
  });

  it("an attacker cannot forge a command link merely by writing command: text (it is not one of ours)", () => {
    // Even if a malicious, unescaped `command:` string somehow appeared in
    // untrusted content, VS Code would only execute it if its command id is
    // in `enabledCommands` — which is derived solely from this builder's own
    // `addCommandLink` calls, never from untrusted text.
    const content = new HoverContentBuilder()
      .addCommandLink("Open details", "gitsail.openCommitDetails", { hash: "abc" })
      .addUntrustedLine("[pwn](command:workbench.action.terminal.new)")
      .build();
    expect(content.enabledCommands).toEqual(["gitsail.openCommitDetails"]);
  });
});
