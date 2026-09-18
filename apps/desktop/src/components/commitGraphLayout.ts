// Pure geometry for rendering the commit graph as SVG connectors (US-067),
// kept separate from `CommitGraph.vue` so it is unit-testable without
// mounting a component — this project's Desktop test setup only has
// store/service-level tests (see `services/repository.test.ts`,
// `stores/session.test.ts`), not a DOM-mounting library.
//
// `rowConnectors` never invents or recomputes a lane/edge: every connector
// it produces is a direct geometric mapping of one row's own
// `passthroughLanes` entry or one of its `edges` — exactly what the Core
// (Rust, `gitsail_domain::graph`) already calculated (US-067 criterion 3).
// This is also what a test can assert against the Core's own output: for
// each row, the number and `resolved` state of "edge" connectors must
// match `row.edges` one-for-one.

import type { CommitGraphRowDto } from "../services/dto";

export type ConnectorKind = "vertical" | "diagonal";

export interface Connector {
  kind: ConnectorKind;
  x1: number;
  x2: number;
  y1: number;
  y2: number;
  // Echoes the source edge's `resolved` flag (always `true` for a
  // passthrough connector, which is never a continuation marker — only an
  // edge can be unresolved).
  resolved: boolean;
}

export interface NodeGlyph {
  hash: string;
  lane: number;
  cx: number;
  cy: number;
  kind: "root" | "merge" | "normal";
}

export const DEFAULT_ROW_HEIGHT = 28;
export const DEFAULT_LANE_WIDTH = 18;

function laneX(lane: number, laneWidth: number): number {
  return lane * laneWidth + laneWidth / 2;
}

// The node glyph for one row, positioned within a `rows`-relative
// coordinate space (row `index` at `y = index * rowHeight + rowHeight/2`).
export function nodeGlyph(
  row: CommitGraphRowDto,
  index: number,
  rowHeight: number = DEFAULT_ROW_HEIGHT,
  laneWidth: number = DEFAULT_LANE_WIDTH,
): NodeGlyph {
  const kind: NodeGlyph["kind"] = row.commit.isMerge
    ? "merge"
    : row.commit.isRoot
      ? "root"
      : "normal";
  return {
    hash: row.commit.hash,
    lane: row.lane,
    cx: laneX(row.lane, laneWidth),
    cy: index * rowHeight + rowHeight / 2,
    kind,
  };
}

// Every connector line for `rows`, in row order. A passthrough lane draws
// a straight vertical segment across the row's full height; each edge
// draws a segment from this row's own lane down to `edge.toLane` on the
// next row — vertical when the lane does not change (the common case: a
// mainline continuation), diagonal otherwise (a branch spawning or a merge
// converging).
export function rowConnectors(
  rows: CommitGraphRowDto[],
  rowHeight: number = DEFAULT_ROW_HEIGHT,
  laneWidth: number = DEFAULT_LANE_WIDTH,
): Connector[] {
  const connectors: Connector[] = [];

  rows.forEach((row, index) => {
    const y1 = index * rowHeight + rowHeight / 2;
    const y2 = y1 + rowHeight;

    for (const lane of row.passthroughLanes) {
      const x = laneX(lane, laneWidth);
      connectors.push({ kind: "vertical", x1: x, x2: x, y1, y2, resolved: true });
    }

    for (const edge of row.edges) {
      const x1 = laneX(row.lane, laneWidth);
      const x2 = laneX(edge.toLane, laneWidth);
      connectors.push({
        kind: x1 === x2 ? "vertical" : "diagonal",
        x1,
        x2,
        y1,
        y2,
        resolved: edge.resolved,
      });
    }
  });

  return connectors;
}

// The total canvas height needed to render `rows.length` rows — used to
// size the virtualized scroll container's spacer (US-067 criterion 2).
export function totalHeight(rowCount: number, rowHeight: number = DEFAULT_ROW_HEIGHT): number {
  return rowCount * rowHeight;
}

// The `[start, end)` row index range visible for a given scroll position —
// the core of the manual virtualization `CommitGraph.vue` uses instead of
// rendering every loaded row's DOM/SVG nodes at once (US-067 criterion 2).
// `overscan` rows are added on each side so scrolling never shows a blank
// flash before the next frame renders.
export function visibleRange(
  scrollTop: number,
  viewportHeight: number,
  rowCount: number,
  rowHeight: number = DEFAULT_ROW_HEIGHT,
  overscan = 4,
): { start: number; end: number } {
  if (rowCount === 0) {
    return { start: 0, end: 0 };
  }
  const first = Math.floor(scrollTop / rowHeight) - overscan;
  const visibleCount = Math.ceil(viewportHeight / rowHeight) + overscan * 2;
  const start = Math.max(0, first);
  const end = Math.min(rowCount, start + visibleCount);
  return { start, end };
}
