// Wires Git availability, the git client, repository context resolution,
// workspace trust, and document sync-state together, and owns the
// extension's lifecycle (T-201, T-202, T-203, T-204). `extension.ts` is the
// only place that touches the real `vscode` module; everything here is
// driven purely through `ExtensionHost` (see `hostTypes.ts`) and is unit
// tested with a plain object implementing it, no `vscode` mock module
// required.
//
// ADR-025 changed what this controller has to establish before it can
// answer anything. It used to locate a `gitsail` binary (on PATH or via
// `gitsail.binaryPath`), verify its version, and only then build a client.
// There is no such binary now: it verifies that `git` itself runs, and
// that the workspace is trusted.

import { GitAvailability, GitClient, probeGit } from "./git/gitClient";
import { classifyDocumentSyncState, describeSyncState } from "./documentState";
import { ExtensionHost, TextEditorLike } from "./hostTypes";
import { RepositoryContext, RepositoryContextResolver } from "./repositoryContext";

const CONFIG_SECTION = "gitsail";

/**
 * Availability of the one external dependency this extension has left.
 *
 * `blocked-untrusted` is a stronger gate than the one it replaces, and
 * deliberately so. Previously only the *configured binary path* was gated
 * on workspace trust, because that setting could come from a
 * `.vscode/settings.json` someone else committed. With that setting gone,
 * the remaining workspace-controlled input is the repository itself — and
 * running `git` inside a repository means honoring that repository's own
 * configuration. So GitSail now runs no Git query at all until the
 * workspace is trusted, which is exactly what this extension's
 * `capabilities.untrustedWorkspaces` description in `package.json` has
 * always promised users.
 */
export type GitProbeResult = GitAvailability | { status: "blocked-untrusted" };

export class ExtensionController {
  private disposables: { dispose(): void }[] = [];
  private disposed = false;
  /** Bumped on every refresh — mirrors the session "epoch" guard
   * `apps/desktop/src-tauri/src/state.rs` already uses for the exact same
   * reason (SAD §26): a refresh that finishes after a newer one has
   * started must never update the UI for the context nobody is looking at
   * anymore, and `dispose()` bumps it too so a callback landing after
   * disposal is guaranteed to see a stale generation. */
  private generation = 0;
  /** Cached across plain active-editor switches — switching files never
   * changes whether `git` is installed, so re-probing it on every editor
   * focus change would be both wasteful and, if it is failing, would
   * re-arm the "surface once" gate below. Cleared only by
   * `invalidateGitProbe()`, on workspace trust grants and configuration
   * changes. */
  private cachedProbe: GitProbeResult | undefined;
  private resolver: RepositoryContextResolver | undefined;
  /** The same client instance handed to `resolver` above — kept alongside
   * it (rather than reconstructed) so `onRepositoryContextChanged`
   * listeners (EPIC-15's `HistoryController`) share the exact same client
   * this controller already validated. */
  private gitClient: GitClient | undefined;
  private notifiedGitProblemThisGeneration = false;
  private readonly repositoryContextListeners = new Set<
    (client: GitClient | undefined, repoRoot: string | undefined) => void
  >();

  private readonly outputChannel;
  private readonly statusBarItem;

  constructor(private readonly host: ExtensionHost) {
    this.outputChannel = host.createOutputChannel("GitSail");
    this.statusBarItem = host.createStatusBarItem();
    this.disposables.push(this.outputChannel, this.statusBarItem);
  }

  async activate(): Promise<void> {
    this.disposables.push(
      this.host.onDidChangeActiveTextEditor(() => this.onActiveEditorChanged()),
      this.host.onDidChangeWorkspaceFolders(() => this.onWorkspaceFoldersChanged()),
      this.host.onDidGrantWorkspaceTrust(() => this.onEnvironmentChanged("workspace trust was granted")),
      this.host.onDidChangeConfiguration((section) => {
        if (section === CONFIG_SECTION) {
          this.onEnvironmentChanged("GitSail configuration changed");
        }
      }),
    );
    await this.refresh();
  }

  /**
   * Tears everything down (T-204 criterion 1): every subscription this
   * controller registered, the output channel, the status bar item, and —
   * by bumping `generation` first — any pending query, so a callback that
   * lands after `dispose()` has already run is guaranteed to see a stale
   * generation and do nothing, instead of touching a disposed status bar
   * item.
   */
  dispose(): void {
    this.disposed = true;
    this.generation++;
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables = [];
    this.resolver = undefined;
    this.notifyRepositoryContext(undefined);
    this.repositoryContextListeners.clear();
  }

  /** A plain editor switch: which repository answers a query root does not
   * change, so this only re-runs the (cached) lookup for the newly active
   * document — it never invalidates the Git probe or the "already
   * notified" gate (US-068 criterion 3). */
  private onActiveEditorChanged(): void {
    this.outputChannel.appendLine("The active editor changed; refreshing repository context.");
    void this.refresh();
  }

  /** The workspace-folder set changed: cached per-root repository answers
   * may no longer apply (a folder could have been removed/replaced), but
   * whether `git` runs is unaffected. */
  private onWorkspaceFoldersChanged(): void {
    this.resolver?.invalidateAll();
    this.outputChannel.appendLine("Workspace folders changed; refreshing repository context.");
    void this.refresh();
  }

  /** Workspace trust was granted, or GitSail configuration changed: both
   * can change whether queries run at all, so the cached probe and the
   * "already notified" gate reset here. */
  private onEnvironmentChanged(reason: string): void {
    this.invalidateGitProbe();
    this.resolver?.invalidateAll();
    this.outputChannel.appendLine(`${reason}; refreshing repository context.`);
    void this.refresh();
  }

  private invalidateGitProbe(): void {
    this.cachedProbe = undefined;
    this.resolver = undefined;
    this.gitClient = undefined;
    this.notifiedGitProblemThisGeneration = false;
  }

  /**
   * Notifies a listener of the current repository binding (a git client
   * plus the repository root it is scoped to) every time it changes,
   * including to `undefined` when there is no usable repository context
   * (no active file, no repository, or Git itself is unavailable) —
   * `HistoryController` (EPIC-15) uses this instead of resolving a
   * repository itself, matching the layering `hostTypes.ts`'s own doc
   * comment describes ("the extension owns: decorations, hover UI,
   * commands... it does not own repository discovery"). Fires once
   * immediately with the current binding so a listener registered after
   * activation is not stuck waiting for the next change.
   */
  onRepositoryContextChanged(
    listener: (client: GitClient | undefined, repoRoot: string | undefined) => void,
  ): { dispose(): void } {
    this.repositoryContextListeners.add(listener);
    listener(this.currentRepoRoot !== undefined ? this.gitClient : undefined, this.currentRepoRoot);
    return { dispose: () => this.repositoryContextListeners.delete(listener) };
  }

  private currentRepoRoot: string | undefined;

  /** `repoRoot: undefined` means "no usable binding" — the client is never
   * handed to a listener in that case, even if `this.gitClient` itself is
   * still set (e.g. the active file simply has no repository), so a
   * listener can treat `client === undefined` as the one authoritative
   * "nothing to query" signal. */
  private notifyRepositoryContext(repoRoot: string | undefined): void {
    this.currentRepoRoot = repoRoot;
    const client = repoRoot !== undefined ? this.gitClient : undefined;
    for (const listener of this.repositoryContextListeners) {
      listener(client, repoRoot);
    }
  }

  /** Where to run `git --version`. Any directory that exists will do — the
   * probe asks nothing about a repository — so this prefers a workspace
   * folder and falls back to the extension host's own working directory
   * rather than refusing to probe when no folder is open. */
  private probeRoot(): string {
    return this.host.workspaceFolders[0]?.uri.fsPath ?? process.cwd();
  }

  private async ensureProbe(): Promise<GitProbeResult> {
    if (!this.cachedProbe) {
      this.cachedProbe = this.host.isWorkspaceTrusted
        ? await probeGit(this.probeRoot())
        : { status: "blocked-untrusted" };
    }
    return this.cachedProbe;
  }

  private async refresh(): Promise<void> {
    const myGeneration = ++this.generation;

    const probe = await this.ensureProbe();
    if (this.isStale(myGeneration)) {
      return;
    }

    if (probe.status !== "ok") {
      this.applyGitProblem(probe);
      return;
    }

    if (!this.resolver) {
      this.gitClient = new GitClient();
      this.resolver = new RepositoryContextResolver(this.gitClient);
    }

    const editor = this.host.activeTextEditor;
    this.logDocumentSyncNote(editor);

    const queryRoot = this.resolver.resolveQueryRoot(editor?.document.uri, this.host.workspaceFolders);
    if (!queryRoot) {
      this.applyContext({ kind: "no-context" }, myGeneration);
      return;
    }

    const context = await this.resolver.resolve(queryRoot);
    this.applyContext(context, myGeneration);
  }

  private logDocumentSyncNote(editor: TextEditorLike | undefined): void {
    const note = describeSyncState(classifyDocumentSyncState(editor?.document));
    if (note) {
      this.outputChannel.appendLine(note);
    }
  }

  private isStale(generation: number): boolean {
    return this.disposed || generation !== this.generation;
  }

  private applyContext(context: RepositoryContext, generation: number): void {
    if (this.isStale(generation)) {
      // A refresh finished after something newer superseded it — never let
      // this late result update the status bar for a context nobody is
      // looking at anymore (T-204 criterion 1).
      return;
    }
    switch (context.kind) {
      case "repository": {
        const branch = context.repository.currentBranch ?? "(detached HEAD)";
        this.statusBarItem.text = `GitSail: ${branch}`;
        this.statusBarItem.tooltip = context.repository.rootPath;
        this.statusBarItem.show();
        this.notifyRepositoryContext(context.repository.rootPath);
        return;
      }
      case "no-repository":
      case "no-context":
        this.statusBarItem.hide();
        this.notifyRepositoryContext(undefined);
        return;
      case "git-unavailable":
        this.statusBarItem.hide();
        this.notifyGitProblemOnce(context.error.message);
        this.notifyRepositoryContext(undefined);
        return;
    }
  }

  private applyGitProblem(probe: GitProbeResult): void {
    this.notifyRepositoryContext(undefined);
    const message = describeGitProbeProblem(probe);
    if (message) {
      this.notifyGitProblemOnce(message);
    }
  }

  /**
   * Surfaces at most one warning toast per standing problem (US-068
   * criterion 3 / US-069 criterion 2: no repeated notification spam) — see
   * `notifiedGitProblemThisGeneration`'s reset points above for exactly
   * when a new occurrence is allowed to notify again. The full detail
   * always goes to the output channel, which the user can open on demand,
   * regardless of whether a toast was also shown.
   */
  private notifyGitProblemOnce(message: string): void {
    this.outputChannel.appendLine(message);
    if (!this.notifiedGitProblemThisGeneration) {
      this.notifiedGitProblemThisGeneration = true;
      this.host.showWarningMessage(message);
    }
  }
}

export function describeGitProbeProblem(probe: GitProbeResult): string | undefined {
  switch (probe.status) {
    case "ok":
      return undefined;
    case "blocked-untrusted":
      return "GitSail is inactive because this workspace is not trusted. Running Git here would honor this repository's own configuration, so GitSail waits for you to trust the workspace first.";
    case "not-found":
      return 'GitSail could not run "git". Install Git, or make sure it is on the PATH that VS Code sees, then reload the window.';
    case "unrecognized":
      return '"git" did not report a recognizable Git version. Check that the "git" on your PATH is really Git, not a different program.';
  }
}
