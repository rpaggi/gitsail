import { describe, expect, it } from "vitest";

import { formatGitTimestamp } from "./timestampFormat";

describe("formatGitTimestamp", () => {
  it("renders the date in the commit's own recorded offset, not UTC", () => {
    // 2023-11-14T22:13:20Z, shown in a +180 minute (UTC+3) offset — the
    // shift crosses into the next calendar day.
    const text = formatGitTimestamp({ secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 180 });

    expect(text).toBe("2023-11-15");
  });

  it("renders the same instant differently for a negative offset", () => {
    const text = formatGitTimestamp({ secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: -180 });

    expect(text).toBe("2023-11-14");
  });

  it("pads single-digit month and day with a leading zero", () => {
    // 2024-01-05T00:00:00Z.
    const text = formatGitTimestamp({ secondsSinceEpoch: 1_704_412_800, utcOffsetMinutes: 0 });

    expect(text).toBe("2024-01-05");
  });
});
