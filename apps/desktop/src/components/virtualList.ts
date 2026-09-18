// A minimal fixed-row-height virtualization primitive (US-057 DoD: "listas
// extensas não exigem render total"). Deliberately not a library
// dependency — the diff viewer's rows are plain, uniform-height text
// lines, so a fixed-height windowing calculation is enough and keeps this
// testable as a pure function, matching this project's
// `commitGraphLayout.ts`/`diffPresentation.ts` convention of extracting
// presentation math out of `.vue` files.

export interface VisibleRange {
  /** Inclusive first rendered index. */
  startIndex: number;
  /** Exclusive end (one past the last rendered index). */
  endIndex: number;
  /** Pixels of empty space to reserve above the rendered slice, so the
   * scrollbar/scroll position behaves as if every row were rendered. */
  offsetTop: number;
  /** Pixels of empty space to reserve below the rendered slice. */
  offsetBottom: number;
}

/**
 * Computes which row indices should actually be rendered for a scrolled
 * viewport over `totalCount` fixed-height rows. `overscan` extra rows are
 * kept rendered on each side of the viewport so a small scroll never shows
 * a blank flash before the next frame renders.
 */
export function computeVisibleRange(params: {
  totalCount: number;
  rowHeight: number;
  viewportHeight: number;
  scrollTop: number;
  overscan?: number;
}): VisibleRange {
  const { totalCount, rowHeight, viewportHeight, scrollTop } = params;
  const overscan = params.overscan ?? 5;

  if (totalCount <= 0 || rowHeight <= 0) {
    return { startIndex: 0, endIndex: 0, offsetTop: 0, offsetBottom: 0 };
  }

  const firstVisible = Math.floor(scrollTop / rowHeight);
  const visibleCount = Math.ceil(viewportHeight / rowHeight) + 1;

  const startIndex = Math.max(0, firstVisible - overscan);
  const endIndex = Math.min(totalCount, firstVisible + visibleCount + overscan);

  return {
    startIndex,
    endIndex,
    offsetTop: startIndex * rowHeight,
    offsetBottom: (totalCount - endIndex) * rowHeight,
  };
}
