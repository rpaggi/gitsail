import { describe, expect, it } from "vitest";

import { AVATAR_COLOR_COUNT, authorColorIndex, authorInitials } from "./authorAvatar";

function author(name: string, email = "") {
  return { name, email };
}

describe("authorInitials", () => {
  it("takes the first and last word of a display name", () => {
    expect(authorInitials(author("Ada Lovelace"))).toBe("AL");
    expect(authorInitials(author("Ada Byron King Lovelace"))).toBe("AL");
  });

  it("uses a single initial for a one-word name", () => {
    expect(authorInitials(author("ada"))).toBe("A");
  });

  it("ignores punctuation inside a name", () => {
    expect(authorInitials(author("  jane (bot) "))).toBe("JB");
  });

  it("falls back to the email local part when the name is unusable", () => {
    expect(authorInitials(author("", "ada.lovelace@example.com"))).toBe("AL");
    expect(authorInitials(author("---", "grace-hopper@example.com"))).toBe("GH");
  });

  it("returns a placeholder rather than an empty string for a malformed author", () => {
    expect(authorInitials(author("", ""))).toBe("?");
    expect(authorInitials(author("!!!", "@@@"))).toBe("?");
  });

  it("does not assume Latin letters", () => {
    expect(authorInitials(author("Ада Лавлейс"))).toBe("АЛ");
  });
});

describe("authorColorIndex", () => {
  it("always lands inside the palette", () => {
    for (const name of ["a", "bb", "ccc", "Ada Lovelace", "", "🙂"]) {
      const index = authorColorIndex(author(name, `${name}@example.com`));
      expect(index).toBeGreaterThanOrEqual(0);
      expect(index).toBeLessThan(AVATAR_COLOR_COUNT);
    }
  });

  it("is stable across calls", () => {
    const a = author("Ada Lovelace", "ada@example.com");
    expect(authorColorIndex(a)).toBe(authorColorIndex(a));
  });

  it("keys on the email, so one person spelled three ways keeps one color", () => {
    const canonical = authorColorIndex(author("Ada Lovelace", "ada@example.com"));
    expect(authorColorIndex(author("ada", "ada@example.com"))).toBe(canonical);
    expect(authorColorIndex(author("A. Lovelace", "ADA@example.com"))).toBe(canonical);
  });

  it("falls back to the name when there is no email", () => {
    expect(authorColorIndex(author("Ada Lovelace", ""))).toBe(
      authorColorIndex(author("ada lovelace", "")),
    );
  });

  it("spreads distinct authors across more than one color", () => {
    const indexes = new Set(
      ["ada", "grace", "alan", "katherine", "margaret", "barbara", "joan", "jean"].map((n) =>
        authorColorIndex(author(n, `${n}@example.com`)),
      ),
    );
    expect(indexes.size).toBeGreaterThan(1);
  });
});
