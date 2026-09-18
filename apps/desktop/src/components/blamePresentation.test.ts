import { describe, expect, it } from "vitest";

import { abbreviateHash, isUncommittedLine, ZERO_COMMIT_HASH } from "./blamePresentation";
import type { BlameLineDto } from "../services/dto";

function line(overrides: Partial<BlameLineDto> = {}): BlameLineDto {
  return {
    finalLine: 1,
    originalLine: 1,
    commit: "a".repeat(40),
    author: { name: "Ada", email: "ada@example.com" },
    timestamp: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
    content: "line content",
    origin: "committed",
    ...overrides,
  };
}

describe("isUncommittedLine", () => {
  it("is false for an ordinary committed line", () => {
    expect(isUncommittedLine(line())).toBe(false);
  });

  it("is true when origin is local", () => {
    expect(isUncommittedLine(line({ origin: "local", commit: ZERO_COMMIT_HASH }))).toBe(true);
  });

  it("is true when the commit is the zero-hash sentinel, even if origin were somehow committed", () => {
    expect(isUncommittedLine(line({ commit: ZERO_COMMIT_HASH }))).toBe(true);
  });
});

describe("abbreviateHash", () => {
  it("defaults to 8 characters", () => {
    expect(abbreviateHash("a".repeat(40))).toBe("aaaaaaaa");
  });

  it("honors an explicit length", () => {
    expect(abbreviateHash("abcdef1234567890", 4)).toBe("abcd");
  });
});
