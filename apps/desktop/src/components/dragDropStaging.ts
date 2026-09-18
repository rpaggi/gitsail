// Drag-and-drop -> mutation mapping for the staged/unstaged file lists
// (US-063/T-196). Kept as one pure function so the mapping is unambiguous
// and testable independent of any DOM drag event: dropping a file that
// came from "unstaged" onto "staged" stages it, the reverse unstages it,
// and any other combination (including dropping back onto its own zone)
// is not a recognized action at all — never guessed at.
//
// This is the *only* drag-and-drop pairing implemented for T-196: both
// directions are Safe-risk (`gitsail_tui::operation::OperationKind::
// StageFiles`/`UnstageFiles`), reversible, and each already has a
// keyboard/click alternative (the existing Stage/Unstage buttons per file
// row) — exactly what US-063 criterion 3 requires before a gesture may be
// enabled at all. Branch switching/deletion, commit reordering, or any
// other candidate drag target were deliberately *not* turned into
// drag-and-drop: none of them has an equally unambiguous, reversible,
// single-step keyboard equivalent, so per US-063 criterion 3 ("se não
// houver uma alternativa clara de teclado... não implemente aquele
// drag/drop específico") they stay button/menu-only.

export type StagingZone = "staged" | "unstaged";

export interface StagingDragPayload {
  path: string;
  sourceZone: StagingZone;
}

export type StagingDropAction = "stage" | "unstage";

/**
 * The MIME type used for the drag payload — a custom type (rather than
 * plain text) so a drop target can distinguish "a GitSail staging row" from
 * an unrelated OS-level drag (e.g. a file dragged in from the desktop),
 * and refuse the latter instead of trying to interpret it as a path.
 */
export const STAGING_DRAG_MIME_TYPE = "application/x-gitsail-staging-entry";

/**
 * Returns the mutation a drop of `payload` onto `targetZone` should
 * perform, or `null` when the drop is not a recognized, unambiguous action
 * — dropping onto the same zone it was dragged from does nothing (US-063
 * criterion 1: every drop target communicates one specific action; a
 * same-zone drop has no action to communicate, so it must not silently
 * do one anyway).
 */
export function dropAction(payload: StagingDragPayload, targetZone: StagingZone): StagingDropAction | null {
  if (payload.sourceZone === targetZone) {
    return null;
  }
  return targetZone === "staged" ? "stage" : "unstage";
}

export function serializeDragPayload(payload: StagingDragPayload): string {
  return JSON.stringify(payload);
}

/** Parses a previously serialized payload, or `null` for anything that is
 * not a well-formed `StagingDragPayload` — including an unrelated drag
 * (e.g. a browser image/text drag) that happens to land on a staging
 * zone. A malformed/foreign payload must never be guessed at as if it
 * were a file path (US-063 criterion 1/3: no ambiguous fallback). */
export function parseDragPayload(raw: string): StagingDragPayload | null {
  try {
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      "path" in parsed &&
      "sourceZone" in parsed &&
      typeof (parsed as { path: unknown }).path === "string" &&
      ((parsed as { sourceZone: unknown }).sourceZone === "staged" ||
        (parsed as { sourceZone: unknown }).sourceZone === "unstaged")
    ) {
      return parsed as StagingDragPayload;
    }
    return null;
  } catch {
    return null;
  }
}
