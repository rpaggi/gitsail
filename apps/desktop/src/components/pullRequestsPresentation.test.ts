import { describe, expect, it } from "vitest";

import {
  pullRequestAuthorLabel,
  pullRequestBranchSummary,
  pullRequestStateLabel,
} from "./pullRequestsPresentation";
// Vite's `?raw` suffix imports the file's source text directly — used only
// to statically guard the template below, never executed as a component.
import panelSource from "./PullRequestsPanel.vue?raw";

describe("pullRequestStateLabel", () => {
  it("maps every state to its label", () => {
    expect(pullRequestStateLabel("open")).toBe("Open");
    expect(pullRequestStateLabel("merged")).toBe("Merged");
    expect(pullRequestStateLabel("closed")).toBe("Closed");
  });
});

describe("pullRequestBranchSummary", () => {
  it("joins both branches when both are present", () => {
    expect(pullRequestBranchSummary("feature/fix", "main")).toBe("feature/fix → main");
  });

  it("is null when either branch is missing, never a partial/blank summary", () => {
    expect(pullRequestBranchSummary(null, "main")).toBeNull();
    expect(pullRequestBranchSummary("feature/fix", null)).toBeNull();
    expect(pullRequestBranchSummary(null, null)).toBeNull();
  });
});

describe("pullRequestAuthorLabel", () => {
  it("returns the author when present", () => {
    expect(pullRequestAuthorLabel("octocat")).toBe("octocat");
  });

  it("falls back to an explicit unknown label, never an empty string", () => {
    expect(pullRequestAuthorLabel(null)).toBe("(unknown)");
  });
});

// US-103 criterion 3: repository/forge-authored content (title, author,
// branch names) must never be rendered as active HTML/Markdown. This
// codebase has no component-mount test harness (`@vue/test-utils` is not a
// dependency here — every other `components/*.ts` presentation helper is
// tested the same way, without mounting the `.vue` file), so this is a
// static regression guard instead: `PullRequestsPanel.vue`'s template must
// never gain a `v-html` binding, which is the one thing that would turn
// Vue's default (escaping) text interpolation into an active-rendering
// path for the untrusted `title`/`author`/branch strings this panel
// displays.
describe("PullRequestsPanel.vue template", () => {
  it("never uses v-html anywhere (untrusted content stays text-interpolated)", () => {
    // Only the `<template>` block is checked (not `<script>`'s own doc
    // comments, which legitimately *mention* `v-html` to explain why it is
    // never used).
    const templateMatch = panelSource.match(/<template>([\s\S]*)<\/template>/);
    expect(templateMatch).not.toBeNull();
    expect(templateMatch![1]).not.toContain("v-html");
  });
});
