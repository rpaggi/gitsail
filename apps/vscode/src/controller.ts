// Wires binary discovery, CLI client, repository context resolution,
// workspace trust, and document sync-state together, and owns the
// extension's lifecycle (T-201, T-202, T-203, T-204). `extension.ts` is the
// only place that touches the real `vscode` module; everything here is
// driven purely through `ExtensionHost` (see `hostTypes.ts`) and is unit
// tested with a plain object implementing it, no `vscode` mock module
// required.

import { GitSailCliClient } from "./cliClient";
import { CliProbeResult, MINIMUM_SUPPORTED_CLI_VERSION, probeCliBinary } from "./cliLocator";
import { classifyDocumentSyncState, describeSyncState } from "./documentState";
import { ExtensionHost, TextEditorLike } from "./hostTypes";
import { RepositoryContext, RepositoryContextResolver } from "./repositoryContext";

const CONFIG_SECTION = "gitsail";
const BINARY_PATH_KEY = "binaryPath";

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
   * changes whether the CLI binary is present/compatible, so re-probing it
   * (spawning `--version`) on every editor focus change would be both
   * wasteful and, if it is failing, would re-arm the "surface once" gate
   * below. Cleared only by `invalidateBinaryProbe()`, on workspace trust
   * grants and `gitsail.binaryPath` configuration changes — the only two
   * things that can actually change this answer. */
  private cachedProbe: CliProbeResult | undefined;
  private resolver: RepositoryContextResolver | undefined;
  /** The same client instance handed to `resolver` above — kept alongside
   * it (rather than reconstructed) so `onRepositoryContextChanged`
   * listeners (EPIC-15's `HistoryController`) share the exact same CLI
   * client/binary resolution this controller already validated, instead of
   * re-probing the binary a second time. */
  private cliClient: GitSailCliClient | undefined;
  private notifiedCliProblemThisGeneration = false;
  private readonly repositoryContextListeners = new Set<
    (client: GitSailCliClient | undefined, repoRoot: string | undefined) => void
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
   * document — it never invalidates the binary probe or the "already
   * notified" gate (US-068 criterion 3). */
  private onActiveEditorChanged(): void {
    this.outputChannel.appendLine("The active editor changed; refreshing repository context.");
    void this.refresh();
  }

  /** The workspace-folder set changed: cached per-root repository answers
   * may no longer apply (a folder could have been removed/replaced), but
   * the CLI binary itself is unaffected. */
  private onWorkspaceFoldersChanged(): void {
    this.resolver?.invalidateAll();
    this.outputChannel.appendLine("Workspace folders changed; refreshing repository context.");
    void this.refresh();
  }

  /** Workspace trust was granted, or `gitsail.binaryPath` changed: the only
   * two things that can change which binary is used, so both the cached
   * probe and the "already notified" gate reset here. */
  private onEnvironmentChanged(reason: string): void {
    this.invalidateBinaryProbe();
    this.resolver?.invalidateAll();
    this.outputChannel.appendLine(`${reason}; refreshing repository context.`);
    void this.refresh();
  }

  private invalidateBinaryProbe(): void {
    this.cachedProbe = undefined;
    this.resolver = undefined;
    this.cliClient = undefined;
    this.notifiedCliProblemThisGeneration = false;
  }

  /**
   * Notifies a listener of the current repository binding (a CLI client
   * plus the repository root it is scoped to) every time it changes,
   * including to `undefined` when there is no usable repository context
   * (no active file, no repository, or the CLI itself is unavailable) —
   * `HistoryController` (EPIC-15) uses this instead of resolving a
   * repository or probing the CLI binary itself, matching the layering
   * `hostTypes.ts`'s own doc comment describes ("the extension owns:
   * decorations, hover UI, commands... it does not own repository
   * discovery"). Fires once immediately with the current binding so a
   * listener registered after activation is not stuck waiting for the next
   * change.
   */
  onRepositoryContextChanged(
    listener: (client: GitSailCliClient | undefined, repoRoot: string | undefined) => void,
  ): { dispose(): void } {
    this.repositoryContextListeners.add(listener);
    listener(this.currentRepoRoot !== undefined ? this.cliClient : undefined, this.currentRepoRoot);
    return { dispose: () => this.repositoryContextListeners.delete(listener) };
  }

  private currentRepoRoot: string | undefined;

  /** `repoRoot: undefined` means "no usable binding" — the client is never
   * handed to a listener in that case, even if `this.cliClient` itself is
   * still set (e.g. the active file simply has no repository), so a
   * listener can treat `client === undefined` as the one authoritative
   * "nothing to query" signal. */
  private notifyRepositoryContext(repoRoot: string | undefined): void {
    this.currentRepoRoot = repoRoot;
    const client = repoRoot !== undefined ? this.cliClient : undefined;
    for (const listener of this.repositoryContextListeners) {
      listener(client, repoRoot);
    }
  }

  private async ensureProbe(): Promise<CliProbeResult> {
    if (!this.cachedProbe) {
      const configuration = this.host.getConfiguration(CONFIG_SECTION);
      this.cachedProbe = await probeCliBinary({
        configuredPath: configuration.get<string>(BINARY_PATH_KEY, ""),
        isWorkspaceTrusted: this.host.isWorkspaceTrusted,
      });
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
      this.applyCliProblem(probe);
      return;
    }

    if (!this.resolver) {
      this.cliClient = new GitSailCliClient({ binaryPath: probe.command });
      this.resolver = new RepositoryContextResolver(this.cliClient);
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
      case "cli-unavailable":
        this.statusBarItem.hide();
        this.notifyCliProblemOnce(context.error.message);
        this.notifyRepositoryContext(undefined);
        return;
    }
  }

  private applyCliProblem(probe: CliProbeResult): void {
    this.notifyRepositoryContext(undefined);
    const message = describeCliProbeProblem(probe);
    if (message) {
      this.notifyCliProblemOnce(message);
    }
  }

  /**
   * Surfaces at most one warning toast per standing problem (US-068
   * criterion 3 / US-069 criterion 2: no repeated notification spam) — see
   * `notifiedCliProblemThisGeneration`'s reset points above for exactly
   * when a new occurrence is allowed to notify again. The full detail
   * always goes to the output channel, which the user can open on demand,
   * regardless of whether a toast was also shown.
   */
  private notifyCliProblemOnce(message: string): void {
    this.outputChannel.appendLine(message);
    if (!this.notifiedCliProblemThisGeneration) {
      this.notifiedCliProblemThisGeneration = true;
      this.host.showWarningMessage(message);
    }
  }
}

function describeCliProbeProblem(probe: CliProbeResult): string | undefined {
  switch (probe.status) {
    case "ok":
      return undefined;
    case "blocked-untrusted":
      return 'GitSail: a custom "gitsail.binaryPath" is configured but ignored because this workspace is not trusted. Trust the workspace, or rely on PATH discovery instead.';
    case "not-found":
      return `Could not run the GitSail CLI ("${probe.command}"). Install gitsail so it is on your PATH, or set the "gitsail.binaryPath" setting to its full path.`;
    case "unrecognized":
      return `"${probe.command}" did not report a recognizable GitSail CLI version. Check that "gitsail.binaryPath" (or your PATH) points at the gitsail executable, not a different program.`;
    case "incompatible":
      return `The GitSail CLI at version ${probe.version.raw} is older than the minimum version this extension supports (${MINIMUM_SUPPORTED_CLI_VERSION.raw}). Update gitsail, or point "gitsail.binaryPath" at a compatible build.`;
  }
}
