// Workspace trust gate (T-204 criterion 2).
//
// US-071/T-204 depends on US-110 (EPIC-22 — Security & Safety), which has
// no corresponding epic/task in the board yet (see this task's Takumi
// note). Rather than block on that, this module implements criterion 2
// directly against VS Code's own native workspace trust API
// (`vscode.workspace.isTrusted` / `onDidGrantWorkspaceTrust`), which is
// already sufficient to satisfy the criterion as written: workspace trust
// controls execution of external processes and reading of sensitive local
// configuration. If/when US-110 defines an additional, broader security
// policy, this gate is the single place that would grow to also honor it.

export interface WorkspaceTrustSource {
  readonly isTrusted: boolean;
  onDidGrantWorkspaceTrust(listener: () => void): { dispose(): void };
}

/**
 * Thin wrapper over `vscode.workspace.isTrusted`/`onDidGrantWorkspaceTrust`.
 *
 * Workspace trust is a one-way transition within a running VS Code session
 * — the real API never fires a "revoked" event, a workspace can only
 * become untrusted again by being reopened — so this only ever listens for
 * the grant, matching that contract instead of inventing a revoke path
 * that could never actually fire.
 */
export class TrustGate {
  private trusted: boolean;
  private readonly changeListeners = new Set<() => void>();
  private readonly subscription: { dispose(): void };

  constructor(private readonly source: WorkspaceTrustSource) {
    this.trusted = source.isTrusted;
    this.subscription = source.onDidGrantWorkspaceTrust(() => {
      this.trusted = true;
      for (const listener of this.changeListeners) {
        listener();
      }
    });
  }

  isTrusted(): boolean {
    return this.trusted;
  }

  onDidChange(listener: () => void): { dispose(): void } {
    this.changeListeners.add(listener);
    return { dispose: () => this.changeListeners.delete(listener) };
  }

  dispose(): void {
    this.subscription.dispose();
    this.changeListeners.clear();
  }
}
