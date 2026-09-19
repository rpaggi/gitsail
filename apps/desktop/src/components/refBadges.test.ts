import { describe, expect, it } from "vitest";

import { refBadges } from "./refBadges";
import type { DecorationDto } from "../services/dto";

const all: DecorationDto[] = [
  { kind: "remoteBranch", remote: "origin", branch: "main" },
  { kind: "tag", name: "v1.0.0" },
  { kind: "branch", name: "main" },
  { kind: "head" },
];

describe("refBadges", () => {
  it("returns nothing for an undecorated commit", () => {
    expect(refBadges([], { includeRemote: true })).toEqual([]);
  });

  it("orders HEAD, then local branches, then tags, then remotes", () => {
    expect(refBadges(all, { includeRemote: true }).map((b) => b.kind)).toEqual([
      "head",
      "branch",
      "tag",
      "remote",
    ]);
  });

  it("qualifies a remote branch with its remote name", () => {
    const [badge] = refBadges(
      [{ kind: "remoteBranch", remote: "upstream", branch: "main" }],
      { includeRemote: true },
    );
    expect(badge).toEqual({ kind: "remote", label: "upstream/main" });
  });

  it("drops remote-tracking refs when they are toggled off, keeping everything else", () => {
    const badges = refBadges(all, { includeRemote: false });
    expect(badges.map((b) => b.label)).toEqual(["HEAD", "main", "v1.0.0"]);
  });

  it("keeps the Core's own ordering within a single kind", () => {
    const badges = refBadges(
      [
        { kind: "branch", name: "zeta" },
        { kind: "branch", name: "alpha" },
      ],
      { includeRemote: true },
    );
    expect(badges.map((b) => b.label)).toEqual(["zeta", "alpha"]);
  });
});
