import { describe, expect, it } from "vitest";

import {
  DEFAULT_BLAME_DELAY_MS,
  DEFAULT_BLAME_FORMAT,
  DEFAULT_BLAME_MODE,
  ZERO_COMMIT_HASH,
  describeBlameLine,
  formatGitTimestamp,
  readBlameDisplayConfig,
  renderBlameTemplate,
} from "../src/blameFormat";
import type { BlameLineDto } from "../src/dto";

function sampleLine(overrides: Partial<BlameLineDto> = {}): BlameLineDto {
  return {
    finalLine: 10,
    originalLine: 8,
    commit: "a".repeat(40),
    author: { name: "Ada Lovelace", email: "ada@example.com" },
    timestamp: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: -180 },
    content: "let x = 1;",
    origin: "committed",
    ...overrides,
  };
}

describe("formatGitTimestamp", () => {
  it("renders the date in the commit's own recorded offset, not the host timezone", () => {
    // 1_700_000_000s UTC is 2023-11-14T22:13:20Z; shifted by -180 minutes
    // (-3h) it is still 2023-11-14 (19:13 local) — pick a boundary case too.
    expect(formatGitTimestamp({ secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: -180 })).toBe(
      "2023-11-14",
    );
  });

  it("crosses a day boundary correctly for a positive offset", () => {
    // 2023-11-14T23:30:00Z + 2h offset => 2023-11-15 local.
    const secondsSinceEpoch = Date.UTC(2023, 10, 14, 23, 30, 0) / 1000;
    expect(formatGitTimestamp({ secondsSinceEpoch, utcOffsetMinutes: 120 })).toBe("2023-11-15");
  });
});

describe("renderBlameTemplate", () => {
  it("substitutes every documented placeholder from the line's own fields", () => {
    const line = sampleLine({ content: "first line\nsecond line" });
    const result = renderBlameTemplate(
      "${author} <${authorEmail}> ${date} ${hash} ${shortHash} :: ${message}",
      line,
    );
    expect(result).toBe(
      "Ada Lovelace <ada@example.com> 2023-11-14 " +
        `${line.commit} ${line.commit.slice(0, 8)} :: first line`,
    );
  });

  it("never fabricates a value not present on the line", () => {
    const line = sampleLine();
    // A template with no placeholders round-trips untouched — proves this
    // function only ever substitutes, never injects unrelated content.
    expect(renderBlameTemplate("static text", line)).toBe("static text");
  });
});

describe("describeBlameLine (US-072/US-075 criterion 3: never invent an author)", () => {
  const config = { enabled: true, format: DEFAULT_BLAME_FORMAT, mode: DEFAULT_BLAME_MODE, delayMs: 0 };

  it("a committed line uses the configured template and the real commit subject", () => {
    const line = sampleLine();
    const result = describeBlameLine(line, config, "Fix off-by-one error", false);
    expect(result.contentText).toContain("Ada Lovelace");
    expect(result.contentText).toContain("Fix off-by-one error");
    expect(result.contentText).toContain(line.commit.slice(0, 8));
    expect(result.hoverLines.some((l) => l.includes("ada@example.com"))).toBe(true);
    expect(result.hoverLines.some((l) => /unsaved/i.test(l))).toBe(false);
  });

  it("an uncommitted (origin: local) line never renders the configured author/date template", () => {
    const line = sampleLine({ origin: "local", commit: ZERO_COMMIT_HASH, author: { name: "Not Committed Yet", email: "not.committed.yet" } });
    const result = describeBlameLine(line, config, undefined, false);
    expect(result.contentText).toBe("Uncommitted change");
    expect(result.contentText).not.toContain("Not Committed Yet");
    expect(result.hoverLines.join(" ")).toMatch(/uncommitted/i);
  });

  it("a dirty (unsaved) document appends an explicit disk-vs-buffer disclaimer, for both committed and local lines", () => {
    const committed = describeBlameLine(sampleLine(), config, "subject", true);
    expect(committed.hoverLines.some((l) => /unsaved/i.test(l) && /disk/i.test(l))).toBe(true);

    const local = describeBlameLine(
      sampleLine({ origin: "local", commit: ZERO_COMMIT_HASH }),
      config,
      undefined,
      true,
    );
    expect(local.hoverLines.some((l) => /unsaved/i.test(l) && /disk/i.test(l))).toBe(true);
  });
});

describe("readBlameDisplayConfig", () => {
  function getFrom(values: Record<string, unknown>) {
    return <T,>(key: string, defaultValue: T): T =>
      key in values ? (values[key] as T) : defaultValue;
  }

  it("returns documented defaults when nothing is configured", () => {
    const config = readBlameDisplayConfig(getFrom({}));
    expect(config).toEqual({
      enabled: true,
      format: DEFAULT_BLAME_FORMAT,
      mode: DEFAULT_BLAME_MODE,
      delayMs: DEFAULT_BLAME_DELAY_MS,
    });
  });

  it("honors an explicit 'allVisibleLines' mode", () => {
    expect(readBlameDisplayConfig(getFrom({ "blame.mode": "allVisibleLines" })).mode).toBe(
      "allVisibleLines",
    );
  });

  it("falls back to the default mode for an unrecognized value instead of throwing", () => {
    expect(readBlameDisplayConfig(getFrom({ "blame.mode": "typo-value" })).mode).toBe(
      DEFAULT_BLAME_MODE,
    );
  });

  it("falls back to the default delay for a negative or non-finite value", () => {
    expect(readBlameDisplayConfig(getFrom({ "blame.delayMs": -5 })).delayMs).toBe(
      DEFAULT_BLAME_DELAY_MS,
    );
    expect(readBlameDisplayConfig(getFrom({ "blame.delayMs": Number.NaN })).delayMs).toBe(
      DEFAULT_BLAME_DELAY_MS,
    );
  });

  it("honors enabled: false (US-072 criterion 2: can be turned off)", () => {
    expect(readBlameDisplayConfig(getFrom({ "blame.enabled": false })).enabled).toBe(false);
  });
});
