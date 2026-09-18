import { describe, expect, it } from "vitest";

import { nodeGlyph, rowConnectors, totalHeight, visibleRange } from "./commitGraphLayout";
import type { CommitGraphRowDto } from "../services/dto";

// Fixtures mirror `crates/gitsail-domain/src/graph.rs`'s own
// `a_merge_commit_opens_a_second_lane_that_converges_back` test: a merge
// (parents a, b) where a/b both converge on a shared ancestor x. Building
// the exact same shape here — rather than a Vue-specific example — is what
// lets a test assert this module never reinterprets what the Core (Rust)
// already calculated (US-067 criterion: comparing rendered edges against
// what the Core computed).

function commit(hash: string, isMerge: boolean, isRoot: boolean, subject: string) {
  return {
    hash,
    shortHash: hash.slice(0, 8),
    parents: [],
    author: { name: "Ada", email: "ada@example.com" },
    committer: { name: "Ada", email: "ada@example.com" },
    authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    subject,
    body: "",
    decorations: [],
    isMerge,
    isRoot,
  };
}

// The exact rows `gitsail_domain::graph::CommitGraph::append_page` would
// produce for that fixture (hand-transcribed from the Rust test's
// assertions, not re-derived): merge on lane 0 with two edges (mainline to
// lane 0, second parent to lane 1); a on lane 0 with lane 1 passing
// through; b on lane 1; x on lane 0 resolving both edges.
const CORE_COMPUTED_ROWS: CommitGraphRowDto[] = [
  {
    commit: commit("aa".padEnd(40, "0"), true, false, "merge"),
    lane: 0,
    edges: [
      { fromLane: 0, toLane: 0, target: "a1".padEnd(40, "0"), resolved: true },
      { fromLane: 0, toLane: 1, target: "b1".padEnd(40, "0"), resolved: true },
    ],
    passthroughLanes: [],
  },
  {
    commit: commit("a1".padEnd(40, "0"), false, false, "a"),
    lane: 0,
    edges: [{ fromLane: 0, toLane: 0, target: "c0".padEnd(40, "0"), resolved: true }],
    passthroughLanes: [1],
  },
  {
    commit: commit("b1".padEnd(40, "0"), false, false, "b"),
    lane: 1,
    edges: [{ fromLane: 1, toLane: 0, target: "c0".padEnd(40, "0"), resolved: true }],
    passthroughLanes: [],
  },
  {
    commit: commit("c0".padEnd(40, "0"), false, true, "x"),
    lane: 0,
    edges: [],
    passthroughLanes: [],
  },
];

describe("rowConnectors (data fidelity against the Core-computed layout)", () => {
  it("produces exactly one connector per edge, with the same resolved flag, never fewer or extra", () => {
    const connectors = rowConnectors(CORE_COMPUTED_ROWS);

    const totalCoreEdges = CORE_COMPUTED_ROWS.reduce((sum, row) => sum + row.edges.length, 0);
    const totalPassthrough = CORE_COMPUTED_ROWS.reduce(
      (sum, row) => sum + row.passthroughLanes.length,
      0,
    );
    expect(connectors).toHaveLength(totalCoreEdges + totalPassthrough);

    // Every edge from the merge row must appear as a connector with the
    // same `resolved` value the Core computed for it — no recomputation.
    for (const row of CORE_COMPUTED_ROWS) {
      for (const edge of row.edges) {
        const match = connectors.find(
          (c) => c.resolved === edge.resolved && (edge.toLane !== row.lane) === (c.kind === "diagonal"),
        );
        expect(match).toBeDefined();
      }
    }
  });

  it("marks a lane change as diagonal and a same-lane continuation as vertical", () => {
    const connectors = rowConnectors(CORE_COMPUTED_ROWS);

    // Merge row's second edge (to lane 1) must be diagonal.
    const diagonal = connectors.find((c) => c.kind === "diagonal");
    expect(diagonal).toBeDefined();
    expect(diagonal?.resolved).toBe(true);

    // Merge row's first edge (mainline, lane 0 -> lane 0) must be vertical.
    const verticalCount = connectors.filter((c) => c.kind === "vertical").length;
    expect(verticalCount).toBeGreaterThan(0);
  });

  it("an unresolved edge (a continuation across a page boundary) is never silently dropped", () => {
    const rowsWithAGap: CommitGraphRowDto[] = [
      {
        commit: commit("c2".padEnd(40, "0"), false, false, "c2"),
        lane: 0,
        edges: [{ fromLane: 0, toLane: 0, target: "c1".padEnd(40, "0"), resolved: false }],
        passthroughLanes: [],
      },
    ];

    const connectors = rowConnectors(rowsWithAGap);

    expect(connectors).toHaveLength(1);
    expect(connectors[0].resolved).toBe(false);
  });
});

describe("nodeGlyph", () => {
  it("classifies root/merge/normal commits distinctly, not by color alone", () => {
    const glyphs = CORE_COMPUTED_ROWS.map((row, i) => nodeGlyph(row, i));
    expect(glyphs[0].kind).toBe("merge");
    expect(glyphs[1].kind).toBe("normal");
    expect(glyphs[3].kind).toBe("root");
  });

  it("positions a node's x coordinate by its own lane, not by row order", () => {
    const glyphs = CORE_COMPUTED_ROWS.map((row, i) => nodeGlyph(row, i));
    expect(glyphs[2].lane).toBe(1);
    expect(glyphs[2].cx).toBeGreaterThan(glyphs[0].cx);
  });
});

describe("virtualization geometry", () => {
  it("totalHeight scales linearly with row count", () => {
    expect(totalHeight(0)).toBe(0);
    expect(totalHeight(10, 20)).toBe(200);
  });

  it("visibleRange never exceeds the loaded row count", () => {
    const { start, end } = visibleRange(0, 500, 4, 28, 4);
    expect(start).toBe(0);
    expect(end).toBe(4);
  });

  it("visibleRange follows scroll position and stays within bounds", () => {
    const { start, end } = visibleRange(2800, 300, 1000, 28, 2);
    expect(start).toBeGreaterThan(0);
    expect(end).toBeLessThanOrEqual(1000);
    expect(end).toBeGreaterThan(start);
  });
});
