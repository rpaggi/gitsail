import { describe, expect, it } from "vitest";

import { formatRelativeTime } from "./relativeTime";

const NOW = 1_700_000_000;

function at(secondsAgo: number) {
  return { secondsSinceEpoch: NOW - secondsAgo, utcOffsetMinutes: 0 };
}

describe("formatRelativeTime", () => {
  it("reports anything under a minute as 'just now'", () => {
    expect(formatRelativeTime(at(0), NOW)).toBe("just now");
    expect(formatRelativeTime(at(59), NOW)).toBe("just now");
  });

  it("singularizes a count of one", () => {
    expect(formatRelativeTime(at(60), NOW)).toBe("1 minute ago");
    expect(formatRelativeTime(at(3600), NOW)).toBe("1 hour ago");
    expect(formatRelativeTime(at(86_400), NOW)).toBe("1 day ago");
  });

  it("pluralizes any other count", () => {
    expect(formatRelativeTime(at(120), NOW)).toBe("2 minutes ago");
    expect(formatRelativeTime(at(7200), NOW)).toBe("2 hours ago");
    expect(formatRelativeTime(at(5 * 86_400), NOW)).toBe("5 days ago");
  });

  it("truncates rather than rounds, so a label never claims more elapsed time than has passed", () => {
    expect(formatRelativeTime(at(119), NOW)).toBe("1 minute ago");
    expect(formatRelativeTime(at(7199), NOW)).toBe("1 hour ago");
  });

  it("steps up to months and years at the documented thresholds", () => {
    expect(formatRelativeTime(at(29 * 86_400), NOW)).toBe("29 days ago");
    expect(formatRelativeTime(at(30 * 86_400), NOW)).toBe("1 month ago");
    expect(formatRelativeTime(at(364 * 86_400), NOW)).toBe("12 months ago");
    expect(formatRelativeTime(at(365 * 86_400), NOW)).toBe("1 year ago");
    expect(formatRelativeTime(at(800 * 86_400), NOW)).toBe("2 years ago");
  });

  it("treats a future timestamp as 'just now' rather than reporting negative time", () => {
    expect(formatRelativeTime(at(-3600), NOW)).toBe("just now");
  });

  it("ignores the recorded UTC offset, which shifts the wall-clock reading but not the instant", () => {
    const utc = { secondsSinceEpoch: NOW - 7200, utcOffsetMinutes: 0 };
    const shifted = { secondsSinceEpoch: NOW - 7200, utcOffsetMinutes: -480 };
    expect(formatRelativeTime(shifted, NOW)).toBe(formatRelativeTime(utc, NOW));
  });
});
