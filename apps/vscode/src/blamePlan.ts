// Pure decoration planning for T-205/US-072: given a file's full blame
// result plus which lines are currently "of interest" (the cursor's line,
// or every visible line, per `BlameDisplayConfig.mode`), decides exactly
// which lines get a decoration and what each one says. No `vscode` import;
// `extension.ts` (or a thin adapter) turns each `BlameDecorationPlanEntry`
// into a real `vscode.DecorationOptions`.

import { BlameDecorationText, BlameDisplayConfig, describeBlameLine } from "./blameFormat";
import { BlameLineDto } from "./dto";

export interface VisibleLineRange {
  /** 1-based, inclusive, matching `BlameLineDto.finalLine`'s convention. */
  startLine: number;
  endLine: number;
}

export type BlameDecorationTarget =
  | { mode: "currentLine"; line: number }
  | { mode: "allVisibleLines"; ranges: readonly VisibleLineRange[] };

export interface BlameDecorationPlanEntry {
  /** 1-based line number, matching `BlameLineDto.finalLine`. */
  line: number;
  text: BlameDecorationText;
}

function linesInRange(range: VisibleLineRange): number[] {
  const lines: number[] = [];
  for (let line = range.startLine; line <= range.endLine; line++) {
    lines.push(line);
  }
  return lines;
}

function targetLineSet(target: BlameDecorationTarget): ReadonlySet<number> {
  if (target.mode === "currentLine") {
    return new Set([target.line]);
  }
  return new Set(target.ranges.flatMap(linesInRange));
}

/** Every distinct, non-"uncommitted" commit hash among the lines `target`
 * selects — the minimal set a caller needs a commit subject for (T-205's
 * `${message}` placeholder), never the whole file's worth of commits. */
export function distinctCommitHashesForTarget(
  blameLines: readonly BlameLineDto[],
  target: BlameDecorationTarget,
): string[] {
  const wanted = targetLineSet(target);
  const hashes = new Set<string>();
  for (const line of blameLines) {
    if (wanted.has(line.finalLine) && line.origin !== "local") {
      hashes.add(line.commit);
    }
  }
  return [...hashes];
}

/**
 * Builds the decoration plan for exactly the lines `target` selects
 * (US-072 criterion 2: current-line vs. all-visible-lines mode).
 * `subjectsByHash` is best-effort: a hash missing from it (e.g. the subject
 * fetch is still in flight, or failed) still gets a decoration, just
 * without `${message}` resolved to a real subject — `describeBlameLine`
 * never invents one either way.
 */
export function planBlameDecorations(
  blameLines: readonly BlameLineDto[],
  config: BlameDisplayConfig,
  target: BlameDecorationTarget,
  subjectsByHash: ReadonlyMap<string, string>,
  documentDirty: boolean,
): BlameDecorationPlanEntry[] {
  const wanted = targetLineSet(target);
  const entries: BlameDecorationPlanEntry[] = [];
  for (const line of blameLines) {
    if (!wanted.has(line.finalLine)) {
      continue;
    }
    const subject = subjectsByHash.get(line.commit);
    entries.push({ line: line.finalLine, text: describeBlameLine(line, config, subject, documentDirty) });
  }
  return entries;
}
