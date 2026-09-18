// Generic confirm/run/result state machine for mutating operations
// (T-194/US-061). This is the Desktop-side port of the same domain concept
// `gitsail-tui`'s `crates/gitsail-tui/src/operation.rs` already models —
// `OperationKind`/`OperationState` — rather than a second, unrelated
// "dialog state" invention: confirm-then-run-in-background-then-land-on-a-
// distinct-success-or-failure-state, with cancellation always available
// before anything mutates. The TUI's own Rust module cannot be imported
// into a Vue/TypeScript frontend directly (different runtime), so this is
// a same-shape reimplementation, not a shared crate — every state name and
// transition below is chosen to match `operation.rs` one-to-one:
//
//   Idle -> Confirming -> InProgress -> Succeeded | Failed
//
// Every story that mutates the repository from the Desktop UI (stage/
// unstage, compose a commit, amend, create/switch/delete a branch, a
// drag-and-drop drop target) goes through this one store rather than
// rolling its own confirm/cancel/result handling, so US-061's three
// criteria (show intent+risk+impact before executing; cancelling before
// confirmation never touches the repository; completing/failing updates
// state with a safe, non-generic message) are enforced in exactly one
// place.

import { defineStore } from "pinia";

import { isErrorPayload, type ErrorPayload } from "../services/errors";

export type OperationRisk = "safe" | "moderate" | "destructive";

/**
 * Describes one pending mutation: the exact operation and target (US-061
 * criterion 1 — never a generic "are you sure?"), its Core-assigned risk
 * tier (mirrors `gitsail_tui::operation::OperationKind::risk`; a caller
 * must use the same three tiers SAD §20 defines, never invent its own
 * label), and the actual async action to run once confirmed.
 */
export interface OperationDescriptor {
  /** A stable machine identifier for the kind of operation, e.g.
   * `"stageFiles"`, `"createCommit"`, `"deleteBranch"` — used by tests and
   * by any future telemetry, never shown to the person directly. */
  kind: string;
  risk: OperationRisk;
  /** Human-readable target, e.g. `"branch 'feature/x'"`, `"a new commit"` —
   * shown verbatim in the confirmation dialog. */
  targetLabel: string;
  /** Extra impact/warning text beyond the target label (e.g. US-059's
   * "this replaces the last commit and rewrites history other people may
   * already have" warning for amend, or US-063's "this permanently deletes
   * the branch" for a forced delete). Optional: a Safe operation like
   * staging a file has nothing more to say than its target label. */
  impact?: string;
  /** Runs the mutation itself (typically one `invoke` through a typed
   * service). Rejecting means failure; resolving means success — this
   * store never inspects the resolved value. */
  run: () => Promise<void>;
}

export type OperationStatus = "idle" | "confirming" | "inProgress" | "succeeded" | "failed";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useOperationStore = defineStore("operation", {
  state: () => ({
    status: "idle" as OperationStatus,
    current: null as OperationDescriptor | null,
    error: null as ErrorPayload | null,
  }),
  getters: {
    isBusy: (state): boolean => state.status === "inProgress",
  },
  actions: {
    /**
     * Starts confirmation for `op`, replacing whatever was pending before
     * (a new request always starts from a clean prompt, mirroring
     * `OperationState::begin`). A `"safe"`-risk operation skips the
     * explicit confirmation step and runs immediately — mirrors
     * `gitsail-tui`'s own "Safe operations skip the Confirming step, only
     * Moderate/Destructive ask for a second Enter" rule
     * (`toggling_stage_on_a_worktree_entry_dispatches_stage_files_without_confirmation`),
     * so a frequent, reversible action like staging a file is never
     * gated behind an extra click.
     */
    async request(op: OperationDescriptor): Promise<void> {
      this.current = op;
      this.status = "confirming";
      this.error = null;
      if (op.risk === "safe") {
        await this.confirm();
      }
    },

    /**
     * Cancels a pending confirmation without running anything (US-061
     * criterion 2: the repository is never touched), and doubles as
     * "dismiss" for a terminal (succeeded/failed) result. A no-op while
     * `inProgress` — work already started cannot be un-started from here,
     * matching `OperationState::cancel`.
     */
    cancel(): void {
      if (this.status === "confirming" || this.status === "succeeded" || this.status === "failed") {
        this.status = "idle";
        this.current = null;
        this.error = null;
      }
    },

    /**
     * Moves from `confirming` to `inProgress`, runs the operation, and
     * lands on `succeeded`/`failed` (US-061 criterion 3). A no-op unless a
     * confirmation was actually pending, so a stray call can never start
     * work out of thin air (matches `OperationState::confirm` being a
     * no-op without a pending `Confirming` state).
     */
    async confirm(): Promise<void> {
      if (this.status !== "confirming" || this.current === null) {
        return;
      }
      const op = this.current;
      this.status = "inProgress";
      try {
        await op.run();
        if (this.status === "inProgress") {
          this.status = "succeeded";
        }
      } catch (error) {
        if (this.status === "inProgress") {
          this.status = "failed";
          this.error = toErrorPayload(error);
        }
      }
    },
  },
});
