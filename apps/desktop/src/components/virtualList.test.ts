import { describe, expect, it } from "vitest";

import { computeVisibleRange } from "./virtualList";

describe("computeVisibleRange", () => {
  it("an empty list renders nothing", () => {
    const range = computeVisibleRange({ totalCount: 0, rowHeight: 20, viewportHeight: 400, scrollTop: 0 });

    expect(range).toEqual({ startIndex: 0, endIndex: 0, offsetTop: 0, offsetBottom: 0 });
  });

  it("a list smaller than the viewport renders everything with no offsets", () => {
    const range = computeVisibleRange({ totalCount: 10, rowHeight: 20, viewportHeight: 400, scrollTop: 0 });

    expect(range.startIndex).toBe(0);
    expect(range.endIndex).toBe(10);
    expect(range.offsetTop).toBe(0);
    expect(range.offsetBottom).toBe(0);
  });

  it("a large list at the top only renders the visible window plus overscan", () => {
    const range = computeVisibleRange({
      totalCount: 10_000,
      rowHeight: 20,
      viewportHeight: 400,
      scrollTop: 0,
      overscan: 5,
    });

    expect(range.startIndex).toBe(0);
    // visible rows = ceil(400/20)+1 = 21, + 5 overscan = 26
    expect(range.endIndex).toBe(26);
    expect(range.offsetTop).toBe(0);
    expect(range.offsetBottom).toBe((10_000 - 26) * 20);
  });

  it("scrolling deep into a large list renders only a window around the scroll position", () => {
    const range = computeVisibleRange({
      totalCount: 10_000,
      rowHeight: 20,
      viewportHeight: 400,
      scrollTop: 10_000, // row 500
      overscan: 5,
    });

    expect(range.startIndex).toBe(500 - 5);
    expect(range.endIndex).toBe(500 + 21 + 5);
    expect(range.startIndex).toBeGreaterThan(0);
    expect(range.endIndex).toBeLessThan(10_000);
  });

  it("never renders past the end of the list even near the bottom", () => {
    const range = computeVisibleRange({
      totalCount: 100,
      rowHeight: 20,
      viewportHeight: 400,
      scrollTop: 1800, // row 90, near the end of 100 rows
      overscan: 5,
    });

    expect(range.endIndex).toBeLessThanOrEqual(100);
    expect(range.offsetBottom).toBeGreaterThanOrEqual(0);
  });
});
