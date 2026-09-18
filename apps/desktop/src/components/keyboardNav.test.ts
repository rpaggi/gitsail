import { describe, expect, it } from "vitest";

import { rovingNextIndex, stepIndex, tabTrapIndex } from "./keyboardNav";

describe("stepIndex", () => {
  it("moves by one step within bounds", () => {
    expect(stepIndex(1, 1, 3, false)).toBe(2);
    expect(stepIndex(1, -1, 3, false)).toBe(0);
  });

  it("wraps at either boundary when wrap is true", () => {
    expect(stepIndex(2, 1, 3, true)).toBe(0);
    expect(stepIndex(0, -1, 3, true)).toBe(2);
  });

  it("returns null at either boundary when wrap is false (no movement, not a clamp)", () => {
    expect(stepIndex(2, 1, 3, false)).toBeNull();
    expect(stepIndex(0, -1, 3, false)).toBeNull();
  });

  it("returns null for an empty list", () => {
    expect(stepIndex(0, 1, 0, true)).toBeNull();
  });
});

describe("rovingNextIndex", () => {
  it("horizontal orientation responds to Left/Right and ignores Up/Down", () => {
    expect(rovingNextIndex(0, "ArrowRight", 3, "horizontal")).toBe(1);
    expect(rovingNextIndex(0, "ArrowLeft", 3, "horizontal")).toBe(2); // wraps by default
    expect(rovingNextIndex(0, "ArrowDown", 3, "horizontal")).toBeNull();
    expect(rovingNextIndex(0, "ArrowUp", 3, "horizontal")).toBeNull();
  });

  it("vertical orientation responds to Up/Down and ignores Left/Right", () => {
    expect(rovingNextIndex(0, "ArrowDown", 3, "vertical")).toBe(1);
    expect(rovingNextIndex(0, "ArrowUp", 3, "vertical", false)).toBeNull();
    expect(rovingNextIndex(0, "ArrowLeft", 3, "vertical")).toBeNull();
    expect(rovingNextIndex(0, "ArrowRight", 3, "vertical")).toBeNull();
  });

  it("Home/End jump to the first/last index in either orientation", () => {
    expect(rovingNextIndex(5, "Home", 10, "vertical")).toBe(0);
    expect(rovingNextIndex(0, "End", 10, "vertical")).toBe(9);
    expect(rovingNextIndex(5, "Home", 10, "horizontal")).toBe(0);
    expect(rovingNextIndex(0, "End", 10, "horizontal")).toBe(9);
  });

  it("an unrelated key navigates nothing", () => {
    expect(rovingNextIndex(0, "Enter", 3, "horizontal")).toBeNull();
    expect(rovingNextIndex(0, "a", 3, "vertical")).toBeNull();
  });

  it("non-wrapping vertical list (e.g. a virtualized commit list) stops at the last loaded row instead of jumping back to the first", () => {
    expect(rovingNextIndex(9, "ArrowDown", 10, "vertical", false)).toBeNull();
  });

  it("full keyboard-only traversal of a 3-item horizontal tablist reaches every tab exactly like clicking each one would (US-055 'fluxo sem mouse')", () => {
    const count = 3;
    let current = 0;
    const visited = new Set<number>([current]);
    // Right, Right, Right (wraps back to 0), Left, Home, End: every index
    // must be reachable purely from key events, with no pointer input.
    for (const key of ["ArrowRight", "ArrowRight", "ArrowRight", "ArrowLeft", "Home", "End"]) {
      const next = rovingNextIndex(current, key, count, "horizontal");
      expect(next).not.toBeNull();
      current = next as number;
      visited.add(current);
    }
    expect(visited).toEqual(new Set([0, 1, 2]));
  });
});

describe("tabTrapIndex", () => {
  it("does nothing for a non-Tab key", () => {
    expect(tabTrapIndex(0, { key: "Enter", shiftKey: false }, 3)).toBeNull();
  });

  it("does nothing in the middle of the dialog (native tab order applies)", () => {
    expect(tabTrapIndex(1, { key: "Tab", shiftKey: false }, 3)).toBeNull();
    expect(tabTrapIndex(1, { key: "Tab", shiftKey: true }, 3)).toBeNull();
  });

  it("wraps Tab from the last focusable element back to the first", () => {
    expect(tabTrapIndex(2, { key: "Tab", shiftKey: false }, 3)).toBe(0);
  });

  it("wraps Shift+Tab from the first focusable element back to the last", () => {
    expect(tabTrapIndex(0, { key: "Tab", shiftKey: true }, 3)).toBe(2);
  });

  it("returns null for an empty dialog (nothing to trap focus within)", () => {
    expect(tabTrapIndex(0, { key: "Tab", shiftKey: false }, 0)).toBeNull();
  });
});
