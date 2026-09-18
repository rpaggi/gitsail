import { describe, expect, it } from "vitest";

import { applyEffectiveTheme, resolveEffectiveTheme } from "./theme";

describe("resolveEffectiveTheme", () => {
  it("resolves an explicit dark preference to dark", () => {
    expect(resolveEffectiveTheme("dark")).toBe("dark");
  });

  it("resolves an explicit light preference to light", () => {
    expect(resolveEffectiveTheme("light")).toBe("light");
  });

  it("resolves system (no explicit choice yet) to dark, never to an OS-inferred theme", () => {
    expect(resolveEffectiveTheme("system")).toBe("dark");
  });
});

describe("applyEffectiveTheme", () => {
  it("sets data-theme to the resolved theme", () => {
    const root = document.createElement("html");
    applyEffectiveTheme("light", root);
    expect(root.getAttribute("data-theme")).toBe("light");
  });

  it("sets data-theme explicitly even for dark, rather than leaving it unset", () => {
    const root = document.createElement("html");
    applyEffectiveTheme("dark", root);
    expect(root.getAttribute("data-theme")).toBe("dark");
  });

  it("overwrites a previously applied theme", () => {
    const root = document.createElement("html");
    applyEffectiveTheme("light", root);
    applyEffectiveTheme("dark", root);
    expect(root.getAttribute("data-theme")).toBe("dark");
  });
});
