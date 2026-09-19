import { describe, expect, it } from "vitest";

import { DEFAULT_VIEW, NAV_ITEMS, navItem, resolveView } from "./navigation";

describe("NAV_ITEMS", () => {
  it("has unique ids", () => {
    const ids = NAV_ITEMS.map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("gives every entry a label, an icon and a heading", () => {
    for (const item of NAV_ITEMS) {
      expect(item.label.length).toBeGreaterThan(0);
      expect(item.icon.length).toBeGreaterThan(0);
      expect(item.title.length).toBeGreaterThan(0);
      expect(item.subtitle.length).toBeGreaterThan(0);
    }
  });

  it("marks Issues as the only unavailable view, since nothing implements it yet", () => {
    const unavailable = NAV_ITEMS.filter((item) => !item.available).map((item) => item.id);
    expect(unavailable).toEqual(["issues"]);
  });

  it("keeps Settings reachable with no repository open, so a failed open is recoverable", () => {
    expect(navItem("settings").requiresRepository).toBe(false);
  });

  it("defaults to a view that exists", () => {
    expect(() => navItem(DEFAULT_VIEW)).not.toThrow();
  });
});

describe("navItem", () => {
  it("throws on an unknown id rather than returning undefined", () => {
    // @ts-expect-error — deliberately outside ViewId
    expect(() => navItem("nope")).toThrow();
  });
});

describe("resolveView", () => {
  it("honors the selection when a repository is open", () => {
    for (const item of NAV_ITEMS) {
      expect(resolveView(item.id, true)).toBe(item.id);
    }
  });

  it("falls back to settings for a repository-scoped view with no repository", () => {
    expect(resolveView("overview", false)).toBe("settings");
    expect(resolveView("commits", false)).toBe("settings");
    expect(resolveView("blame", false)).toBe("settings");
  });

  it("honors views that do not need a repository even with none open", () => {
    expect(resolveView("settings", false)).toBe("settings");
    expect(resolveView("issues", false)).toBe("issues");
  });
});
