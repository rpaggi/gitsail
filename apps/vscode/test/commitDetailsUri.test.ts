import { describe, expect, it } from "vitest";

import {
  COMMIT_DETAILS_URI_SCHEME,
  buildCommitDetailsUriString,
  parseCommitDetailsUri,
} from "../src/commitDetailsUri";

describe("buildCommitDetailsUriString / parseCommitDetailsUri (T-206 criterion 2)", () => {
  it("round-trips repo/hash through the URI string", () => {
    const params = { repoRoot: "/workspace/project", hash: "a".repeat(40) };
    const uriString = buildCommitDetailsUriString(params);
    expect(uriString.startsWith(`${COMMIT_DETAILS_URI_SCHEME}:/`)).toBe(true);

    const url = new URL(uriString);
    const parsed = parseCommitDetailsUri({
      scheme: COMMIT_DETAILS_URI_SCHEME,
      path: url.pathname,
      query: url.search.slice(1),
    });
    expect(parsed).toEqual(params);
  });

  it("returns undefined for a different scheme", () => {
    expect(
      parseCommitDetailsUri({ scheme: "file", path: "/x", query: "repo=/r&hash=abc" }),
    ).toBeUndefined();
  });

  it("returns undefined when a required parameter is missing", () => {
    expect(
      parseCommitDetailsUri({ scheme: COMMIT_DETAILS_URI_SCHEME, path: "/x", query: "repo=/r" }),
    ).toBeUndefined();
  });
});
