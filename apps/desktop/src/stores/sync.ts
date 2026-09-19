// Remote sync state (US-060/T-193): fetch/pull/push. Every mutation goes
// through `stores/operation.ts` (T-194/US-061), so a pull/push always shows
// its resolved remote/branch/upstream and risk before running and can
// always be cancelled before it touches the repository — the same
// confirmation flow `stores/branches.ts` already uses for create/switch/
// delete.
//
// Risk classification mirrors `gitsail_tui::operation::OperationKind`
// exactly (T-182/US-049, SAD §20's own named example): `fetch` is Safe, so
// it dispatches without a confirmation step; `pull`/`push` are Moderate, so
// they always confirm first. Before any of the three even reaches the
// confirmation step, `resolveSyncTarget` is called to show which remote
// (and, for pull/push, which branch/upstream) would be affected — US-060
// criterion 2 — and a resolution failure (no remote configured, or an
// ambiguous choice with no upstream) is surfaced immediately, dispatching
// nothing, exactly like `gitsail-tui`'s own `resolve_sync_remote` refusal.

import { defineStore } from "pinia";

import {
  fetch as fetchCommand,
  listRemotes,
  pull as pullCommand,
  push as pushCommand,
  resolveSyncTarget,
} from "../services/sync";
import { getForgeLink, openForgeLink } from "../services/forge";
import type { PullResultDto, RemoteDto, SyncTargetDto } from "../services/dto";
import { isErrorPayload, type ErrorPayload } from "../services/errors";
import { useOperationStore } from "./operation";
import { useRepositorySessionStore } from "./session";

function toErrorPayload(error: unknown): ErrorPayload {
  return isErrorPayload(error) ? error : { code: "internal", message: String(error) };
}

export const useSyncStore = defineStore("sync", {
  state: () => ({
    remotes: [] as RemoteDto[],
    isLoadingRemotes: false,
    /** The most recently resolved sync target, kept on display so the
     * person can see which remote/branch is currently affected even before
     * choosing an action (US-060 criterion 2). */
    resolvedTarget: null as SyncTargetDto | null,
    /** Set when resolving the sync target itself fails (no remote
     * configured, or ambiguous with no upstream) — distinct from
     * `useOperationStore().error`, which only ever reports a failure of an
     * operation that actually ran. */
    resolveError: null as ErrorPayload | null,
    lastFetchResult: null as SyncTargetDto | null,
    lastPullResult: null as PullResultDto | null,
    lastPushResult: null as SyncTargetDto | null,
    /** The repository's web URL on its detected GitHub/GitLab remote
     * (T-243/US-101), or `null` when no configured remote resolves to a
     * known forge — the single source of truth for whether the "open in
     * browser" action is shown at all (US-101 criterion 3: this is never
     * an error state). */
    forgeLink: null as string | null,
  }),
  actions: {
    /** Resolves and records the repository-root forge link, so the UI can
     * show/hide the "open in browser" action without guessing (T-243/
     * US-101). Called once up front (e.g. on mount) alongside
     * `loadRemotes`/`refreshTarget`. */
    async refreshForgeLink(): Promise<void> {
      try {
        this.forgeLink = await getForgeLink({ kind: "repository" });
      } catch {
        // Mirrors `loadRemotes`'s own "display convenience, never blocks
        // anything else" handling — an unresolvable link just hides the
        // action.
        this.forgeLink = null;
      }
    },

    /** Opens the repository's forge link in the browser (T-243/US-101). A
     * no-op when `forgeLink` is `null` — the button calling this is only
     * ever shown when it is not. */
    async openRepositoryForgeLink(): Promise<void> {
      if (this.forgeLink === null) {
        return;
      }
      await openForgeLink({ kind: "repository" });
    },

    async loadRemotes(): Promise<void> {
      this.isLoadingRemotes = true;
      try {
        this.remotes = await listRemotes();
      } catch {
        // Listing configured remotes is a display convenience alongside
        // the resolved target; a failure here is already surfaced by
        // `refreshTarget`'s own error (both read from the same repository
        // state), so this does not duplicate it.
        this.remotes = [];
      } finally {
        this.isLoadingRemotes = false;
      }
    },

    /** Resolves and records which remote/branch a sync action would
     * target, without dispatching anything (US-060 criterion 2). Called
     * once up front (e.g. on mount) to keep a standing display, and again
     * immediately before each `request*` action below so what actually
     * runs is never a stale snapshot. */
    async refreshTarget(): Promise<SyncTargetDto | null> {
      try {
        this.resolvedTarget = await resolveSyncTarget();
        this.resolveError = null;
        return this.resolvedTarget;
      } catch (error) {
        this.resolvedTarget = null;
        this.resolveError = toErrorPayload(error);
        return null;
      }
    },

    /** Requests a fetch (Safe risk — dispatches without a confirmation
     * step, mirroring `gitsail_tui::operation::OperationKind::Fetch`). */
    async requestFetch(): Promise<void> {
      const target = await this.refreshTarget();
      if (target === null) {
        return;
      }
      const operation = useOperationStore();
      await operation.request({
        kind: "fetch",
        risk: "safe",
        promptLabel: `Fetch from remote '${target.remote}'`,
        run: async () => {
          this.lastFetchResult = await fetchCommand();
        },
      });
    },

    /** Requests a fast-forward-only pull (Moderate risk — confirms first).
     * A refused divergence surfaces as an ordinary failed operation; this
     * never merges/rebases/force-integrates on its own. */
    async requestPull(): Promise<void> {
      const target = await this.refreshTarget();
      if (target === null || target.branch === null) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "pull",
        risk: "moderate",
        promptLabel: `Pull branch '${target.branch}' from remote '${target.remote}' (fast-forward only)`,
        run: async () => {
          this.lastPullResult = await pullCommand();
          await session.refreshStatus("after_mutation");
        },
      });
    },

    /** Requests a plain, non-force push (Moderate risk — confirms first).
     * A non-fast-forward rejection is never escalated to a force push. */
    async requestPush(): Promise<void> {
      const target = await this.refreshTarget();
      if (target === null || target.branch === null) {
        return;
      }
      const operation = useOperationStore();
      const session = useRepositorySessionStore();
      await operation.request({
        kind: "push",
        risk: "moderate",
        promptLabel: `Push branch '${target.branch}' to remote '${target.remote}'`,
        run: async () => {
          this.lastPushResult = await pushCommand();
          await session.refreshStatus("after_mutation");
        },
      });
    },
  },
});
