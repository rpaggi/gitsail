import { describe, expect, it } from "vitest";

import { HISTORY_URI_SCHEME, buildHistoryUriString, parseHistoryUri } from "../src/historyUri";

describe("buildHistoryUriString / parseHistoryUri (T-209 criterion 3)", () => {
  it("round-trips repo/path/revision through the URI string", () => {
    const params = { repoRoot: "/workspace/project", filePath: "src/lib.rs", revision: "a".repeat(40) };
    const uriString = buildHistoryUriString(params);
    expect(uriString.startsWith(`${HISTORY_URI_SCHEME}:/src/lib.rs?`)).toBe(true);

    const url = new URL(uriString);
    const parsed = parseHistoryUri({ scheme: HISTORY_URI_SCHEME, path: url.pathname, query: url.search.slice(1) });
    expect(parsed).toEqual(params);
  });

  it("preserves a nested path's extension so language detection still applies", () => {
    const uriString = buildHistoryUriString({
      repoRoot: "/repo",
      filePath: "src/deep/module.test.ts",
      revision: "b".repeat(40),
    });
    expect(uriString.startsWith(`${HISTORY_URI_SCHEME}:/src/deep/module.test.ts?`)).toBe(true);
  });

  it("returns undefined for a different scheme", () => {
    expect(parseHistoryUri({ scheme: "file", path: "/whatever", query: "repo=x&path=y&revision=z" })).toBeUndefined();
  });

  it("returns undefined when a required query parameter is missing", () => {
    expect(
      parseHistoryUri({ scheme: HISTORY_URI_SCHEME, path: "/lib.rs", query: "repo=/repo&path=lib.rs" }),
    ).toBeUndefined();
  });
});
