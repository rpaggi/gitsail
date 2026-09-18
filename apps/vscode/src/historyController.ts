// Orchestrates EPIC-15 (blame decorations, hover, file/line history, commit
// diff, Desktop handoff) against `HistoryHost` (`historyHostTypes.ts`) —
// the same "testable orchestration, no `vscode` import" pattern
// `controller.ts` established for EPIC-14. `extension.ts` is the only file
// that adapts the real `vscode` module to `HistoryHost` and hands it here.
//
// This controller owns no repository-detection or CLI-discovery logic of
// its own (that remains `ExtensionController`'s job, T-201..T-204) — it is
// fed the resolved `GitSailCliClient`/repository root via
// `onRepositoryContextChanged`, wired by `extension.ts` from
// `ExtensionController`'s own hook.

import path from "node:path";

import {
  BlameDecorationTarget,
  distinctCommitHashesForTarget,
  planBlameDecorations,
} from "./blamePlan";
import { readBlameDisplayConfig } from "./blameFormat";
import { BlameQueryCache, CommitSubjectCache, resolveCommitSubjects } from "./blameService";
import { GitSailCliClient } from "./cliClient";
import { describeCliFailure } from "./cliResult";
import {
  buildCommitDetailsUriString,
  parseCommitDetailsUri,
} from "./commitDetailsUri";
import { renderCommitDetailsText } from "./commitDetailsText";
import { describeCommitDiffBase, getCommit, getCommitDiff, getFileContentAtRevision } from "./commitService";
import { CommitDiffDto, FileContentDto } from "./dto";
import {
  DesktopLaunchOutcome,
  describeDesktopLaunchFallback,
  launchDesktopForCommit,
} from "./desktopHandoff";
import { describeFileHistoryOutcome, getFileHistoryPage } from "./fileHistoryService";
import {
  BlameDecorationRenderEntry,
  Disposable,
  HistoryHost,
  HistoryTextEditorLike,
  QuickPickItemLike,
  UriLike,
} from "./historyHostTypes";
import { EMPTY_CONTENT_REVISION, buildHistoryUriString, parseHistoryUri } from "./historyUri";
import { HoverContentBuilder } from "./hoverSanitizer";
import {
  LOAD_MORE_ITEM_ID,
  NO_HISTORY_ITEM_ID,
  buildFileHistoryQuickPickItems,
  buildLineHistoryQuickPickItems,
} from "./historyPresentation";
import { describeLineHistoryBufferCaveat, getLineHistory } from "./lineHistoryService";

export const COMMANDS = {
  toggleBlame: "gitsail.blame.toggle",
  openCommitDetails: "gitsail.openCommitDetails",
  copyCommitHash: "gitsail.copyCommitHash",
  showFileHistory: "gitsail.showFileHistory",
  showLineHistory: "gitsail.showLineHistory",
  openCommitDiffForFile: "gitsail.openCommitDiffForFile",
  openInDesktop: "gitsail.openInDesktop",
} as const;

const CONFIG_SECTION = "gitsail";
const FILE_HISTORY_PAGE_SIZE = 20;

interface RepositoryBinding {
  client: GitSailCliClient;
  repoRoot: string;
}

export class HistoryController {
  private disposables: Disposable[] = [];
  private disposed = false;
  private binding: RepositoryBinding | undefined;
  private blameQueryCache: BlameQueryCache | undefined;
  private commitSubjectCache: CommitSubjectCache | undefined;
  private debounceTimer: ReturnType<typeof setTimeout> | undefined;
  /** Bumped whenever a debounced recompute is superseded — matches
   * `ExtensionController`'s own "generation" guard (T-204) so a stale async
   * result can never apply a decoration for content nobody is looking at
   * anymore. */
  private generation = 0;

  constructor(private readonly host: HistoryHost) {}

  activate(): void {
    this.disposables.push(
      this.host.onDidChangeActiveTextEditor(() => this.scheduleBlameRecompute()),
      this.host.onDidChangeTextEditorSelection(() => this.scheduleBlameRecompute()),
      this.host.onDidChangeVisibleRanges(() => this.scheduleBlameRecompute()),
      this.host.onDidChangeTextDocument(() => {
        // An edit invalidates every previously computed decoration for that
        // document's content version — the next recompute naturally uses a
        // fresh cache key (BlameQueryCache is keyed by document.version),
        // so a stale decoration is never left in place (T-205 DoD).
        this.scheduleBlameRecompute();
      }),
      // Reacts to a `gitsail.blame.*` change made any way (Settings UI,
      // settings.json, or `toggleBlame()` below) — not just the command.
      this.host.onDidChangeConfiguration(() => this.scheduleBlameRecompute()),
      this.host.registerCommand(COMMANDS.toggleBlame, () => this.toggleBlame()),
      this.host.registerCommand(COMMANDS.openCommitDetails, (...args: unknown[]) =>
        this.openCommitDetails((args[0] as { hash: string } | undefined)?.hash),
      ),
      this.host.registerCommand(COMMANDS.copyCommitHash, (...args: unknown[]) =>
        this.copyCommitHash((args[0] as { hash: string } | undefined)?.hash),
      ),
      this.host.registerCommand(COMMANDS.showFileHistory, () => this.showFileHistory()),
      this.host.registerCommand(COMMANDS.showLineHistory, () => this.showLineHistory()),
      this.host.registerCommand(COMMANDS.openCommitDiffForFile, (...args: unknown[]) => {
        const arg = args[0] as { hash: string; filePath?: string } | undefined;
        return arg ? this.openCommitDiffForFile(arg.hash, arg.filePath) : undefined;
      }),
      this.host.registerCommand(COMMANDS.openInDesktop, (...args: unknown[]) =>
        this.openInDesktop((args[0] as { hash: string } | undefined)?.hash),
      ),
      this.host.registerHistoryContentProvider((uri) => this.resolveHistoryContent(uri)),
    );
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    if (this.debounceTimer !== undefined) {
      clearTimeout(this.debounceTimer);
    }
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables = [];
  }

  /** Wired by `extension.ts` from `ExtensionController`'s own repository
   * context hook — this controller never resolves a repository itself. */
  onRepositoryContextChanged(client: GitSailCliClient | undefined, repoRoot: string | undefined): void {
    if (client && repoRoot) {
      this.binding = { client, repoRoot };
      this.blameQueryCache = new BlameQueryCache(client);
      this.commitSubjectCache = new CommitSubjectCache(client);
    } else {
      this.binding = undefined;
      this.blameQueryCache = undefined;
      this.commitSubjectCache = undefined;
    }
    this.scheduleBlameRecompute();
  }

  // -- Blame decorations (T-205/US-072) ------------------------------------

  private scheduleBlameRecompute(): void {
    if (this.debounceTimer !== undefined) {
      clearTimeout(this.debounceTimer);
    }
    const editor = this.host.activeTextEditor;
    if (!editor || !this.binding) {
      return;
    }
    const config = readBlameDisplayConfig((key, fallback) => this.host.getConfiguration(CONFIG_SECTION).get(key, fallback));
    if (!config.enabled) {
      this.host.setBlameDecorations(editor, []);
      return;
    }
    const myGeneration = ++this.generation;
    this.debounceTimer = setTimeout(() => {
      void this.recomputeBlameDecorations(editor, myGeneration);
    }, config.delayMs);
  }

  private async recomputeBlameDecorations(editor: HistoryTextEditorLike, generation: number): Promise<void> {
    if (!this.binding || !this.blameQueryCache || !this.commitSubjectCache) {
      return;
    }
    const config = readBlameDisplayConfig((key, fallback) => this.host.getConfiguration(CONFIG_SECTION).get(key, fallback));
    if (!config.enabled) {
      return;
    }
    const filePath = relativeToRepo(this.binding.repoRoot, editor.document.uri.fsPath);
    const blameResult = await this.blameQueryCache.get(
      { repoRoot: this.binding.repoRoot, filePath },
      editor.document.version,
    );
    if (this.isStale(generation)) {
      return;
    }
    if (blameResult.kind !== "ok") {
      // A blame failure is not surfaced as a decoration error toast per
      // line — it would be far too noisy on every cursor move; the CLI
      // client's own client-level problems are already surfaced once by
      // `ExtensionController`. Silently show no decorations instead.
      this.host.setBlameDecorations(editor, []);
      return;
    }

    const target: BlameDecorationTarget =
      config.mode === "currentLine"
        ? { mode: "currentLine", line: editor.activeLine }
        : { mode: "allVisibleLines", ranges: editor.visibleRanges };

    const hashes = distinctCommitHashesForTarget(blameResult.value.lines, target);
    const subjects = await resolveCommitSubjects(this.commitSubjectCache, this.binding.repoRoot, hashes);
    if (this.isStale(generation)) {
      return;
    }

    const plan = planBlameDecorations(blameResult.value.lines, config, target, subjects, editor.document.isDirty);
    const entries: BlameDecorationRenderEntry[] = plan.map((entry) => {
      const hover = new HoverContentBuilder();
      for (const line of entry.text.hoverLines) {
        hover.addUntrustedLine(line);
      }
      const commitForLine = blameResult.value.lines.find((l) => l.finalLine === entry.line);
      if (commitForLine && commitForLine.origin !== "local") {
        hover
          .addBlankLine()
          .addCommandLink("Open full commit details", COMMANDS.openCommitDetails, { hash: commitForLine.commit })
          .addTrustedLine(" | ")
          // US-076 criterion 2: copying the full hash is reachable directly
          // from the blame hover, not only from the Desktop-handoff
          // fallback — the same command, the same full (never abbreviated)
          // hash `commitForLine.commit` already carries.
          .addCommandLink("Copy commit hash", COMMANDS.copyCommitHash, { hash: commitForLine.commit });
      }
      const built = hover.build();
      return {
        line: entry.line,
        contentText: entry.text.contentText,
        hoverMarkdown: built.markdown,
        hoverEnabledCommands: built.enabledCommands,
      };
    });
    this.host.setBlameDecorations(editor, entries);
  }

  private isStale(generation: number): boolean {
    return this.disposed || generation !== this.generation;
  }

  /**
   * Actually flips the persisted `gitsail.blame.enabled` setting (US-072
   * criterion 2) — reading and negating the *effective* current value
   * (rather than this controller keeping a separate on/off flag) means the
   * toggle command and hand-editing the setting can never disagree about
   * the current state. The resulting `onDidChangeConfiguration` event
   * (registered in `activate()`) is what actually applies/clears
   * decorations — this method only persists the setting and reports it.
   */
  private async toggleBlame(): Promise<void> {
    const config = readBlameDisplayConfig((key, fallback) => this.host.getConfiguration(CONFIG_SECTION).get(key, fallback));
    const next = !config.enabled;
    await this.host.getConfiguration(CONFIG_SECTION).update("blame.enabled", next);
    void this.host.showInformationMessage(
      next ? "GitSail inline blame enabled." : "GitSail inline blame disabled.",
    );
  }

  // -- Commit details / hover (T-206/US-073) -------------------------------

  private async openCommitDetails(hash: string | undefined): Promise<void> {
    if (!hash || !this.binding) {
      return;
    }
    const uri = buildCommitDetailsUriString({ repoRoot: this.binding.repoRoot, hash });
    await this.host.openDocument(uri);
  }

  private async copyCommitHash(hash: string | undefined): Promise<void> {
    if (!hash) {
      return;
    }
    await this.host.writeClipboardText(hash);
    void this.host.showInformationMessage(`Copied commit hash ${hash}.`);
  }

  private async resolveHistoryContent(uri: UriLike): Promise<string> {
    const fileParams = parseHistoryUri(uri);
    if (fileParams) {
      if (fileParams.revision === EMPTY_CONTENT_REVISION) {
        return "";
      }
      if (!this.binding) {
        return "(GitSail is not connected to a repository.)";
      }
      const result = await getFileContentAtRevision(
        this.binding.client,
        fileParams.repoRoot,
        fileParams.filePath,
        fileParams.revision,
      );
      return renderFileContentResult(result.kind === "ok" ? result.value : undefined, result.kind !== "ok" ? describeCliFailure(result) : undefined);
    }

    const commitParams = parseCommitDetailsUri(uri);
    if (commitParams && this.binding) {
      const result = await getCommit(this.binding.client, commitParams.repoRoot, commitParams.hash);
      if (result.kind !== "ok") {
        return `(Could not load commit ${commitParams.hash}: ${describeCliFailure(result)})`;
      }
      return renderCommitDetailsText(result.value);
    }

    return "(Unknown GitSail content.)";
  }

  // -- File history (T-207/US-074) -----------------------------------------

  private async showFileHistory(): Promise<void> {
    const editor = this.host.activeTextEditor;
    if (!editor || !this.binding) {
      void this.host.showWarningMessage("Open a file inside a Git repository to view its history.");
      return;
    }
    const filePath = relativeToRepo(this.binding.repoRoot, editor.document.uri.fsPath);
    await this.browseFileHistoryPage(this.binding, filePath, undefined);
  }

  private async browseFileHistoryPage(
    binding: RepositoryBinding,
    filePath: string,
    cursor: string | undefined,
  ): Promise<void> {
    const result = await getFileHistoryPage(binding.client, {
      repoRoot: binding.repoRoot,
      filePath,
      cursor,
      limit: FILE_HISTORY_PAGE_SIZE,
    });
    if (result.kind !== "ok") {
      void this.host.showErrorMessage(`Could not load history for ${filePath}: ${describeCliFailure(result)}`);
      return;
    }
    const items = buildFileHistoryQuickPickItems(describeFileHistoryOutcome(result.value));
    const selected = await this.host.showQuickPick(items, `History of ${filePath}`);
    if (!selected || selected.id === NO_HISTORY_ITEM_ID) {
      return;
    }
    if (selected.id === LOAD_MORE_ITEM_ID) {
      await this.browseFileHistoryPage(binding, filePath, result.value.nextCursor);
      return;
    }
    await this.openCommitDiffForFile(selected.id, filePath);
  }

  // -- Line history (T-208/US-075) -----------------------------------------

  private async showLineHistory(): Promise<void> {
    const editor = this.host.activeTextEditor;
    if (!editor || !this.binding) {
      void this.host.showWarningMessage("Open a file inside a Git repository to view line history.");
      return;
    }
    const filePath = relativeToRepo(this.binding.repoRoot, editor.document.uri.fsPath);
    const caveat = describeLineHistoryBufferCaveat(editor.document.isDirty);
    if (caveat) {
      void this.host.showWarningMessage(caveat);
    }
    const result = await getLineHistory(this.binding.client, {
      repoRoot: this.binding.repoRoot,
      filePath,
      range: { startLine: editor.selection.startLine, endLine: editor.selection.endLine },
    });
    if (result.kind !== "ok") {
      void this.host.showErrorMessage(`Could not load line history: ${describeCliFailure(result)}`);
      return;
    }
    const items = buildLineHistoryQuickPickItems(result.value);
    const selected = await this.host.showQuickPick(
      items,
      `History of lines ${editor.selection.startLine}-${editor.selection.endLine}`,
    );
    if (!selected || selected.id === NO_HISTORY_ITEM_ID) {
      return;
    }
    await this.openCommitDiffForFile(selected.id, filePath);
  }

  // -- Commit diff / open in native diff editor (T-209/US-076) -------------

  private async openCommitDiffForFile(hash: string, preferredFilePath: string | undefined): Promise<void> {
    if (!this.binding) {
      return;
    }
    const result = await getCommitDiff(this.binding.client, this.binding.repoRoot, hash);
    if (result.kind !== "ok") {
      void this.host.showErrorMessage(`Could not load the diff for ${hash}: ${describeCliFailure(result)}`);
      return;
    }
    const commitDiff = result.value;
    let file = preferredFilePath
      ? commitDiff.diff.files.find((f) => f.path === preferredFilePath || f.previousPath === preferredFilePath)
      : undefined;
    if (!file) {
      if (commitDiff.diff.files.length === 0) {
        void this.host.showInformationMessage(`Commit ${hash.slice(0, 8)} has no file changes to diff.`);
        return;
      }
      if (commitDiff.diff.files.length === 1) {
        file = commitDiff.diff.files[0];
      } else {
        const items = buildCommitDiffFileQuickPickItems(commitDiff);
        const picked = await this.host.showQuickPick(items, `Files changed in ${hash.slice(0, 8)}`);
        if (!picked) {
          return;
        }
        file = commitDiff.diff.files.find((f) => f.path === picked.id);
        if (!file) {
          return;
        }
      }
    }
    if (file.isBinary) {
      void this.host.showInformationMessage(`${file.path} is a binary file; GitSail cannot show a text diff for it.`);
      return;
    }

    const baseRevision = commitDiff.base;
    const leftPath = file.previousPath ?? file.path;
    const leftUri =
      baseRevision === null
        ? buildHistoryUriString({ repoRoot: this.binding.repoRoot, filePath: leftPath, revision: EMPTY_CONTENT_REVISION })
        : buildHistoryUriString({ repoRoot: this.binding.repoRoot, filePath: leftPath, revision: baseRevision });
    const rightUri = buildHistoryUriString({
      repoRoot: this.binding.repoRoot,
      filePath: file.path,
      revision: commitDiff.target,
    });

    const title = `${file.path} (${describeCommitDiffBase(commitDiff)})`;
    await this.host.openDiff(leftUri, rightUri, title);
  }

  // -- Desktop handoff (T-210/US-077) --------------------------------------

  private async openInDesktop(hash: string | undefined): Promise<void> {
    if (!hash || !this.binding) {
      return;
    }
    const desktopPath = this.host.getConfiguration(CONFIG_SECTION).get<string>("desktop.path", "");
    const outcome: DesktopLaunchOutcome = await launchDesktopForCommit(desktopPath, this.binding.repoRoot, hash);
    if (outcome.status === "launched") {
      return;
    }
    const message = describeDesktopLaunchFallback(outcome);
    const action = await this.host.showWarningMessage(message, "Copy commit hash");
    if (action === "Copy commit hash") {
      await this.copyCommitHash(hash);
    }
  }
}

/** `path.relative` (not manual string-prefix slicing) so this is correct on
 * Windows (drive letters, backslashes) the same way `repositoryContext.ts`'s
 * `isUnderPath` already is — then normalized to forward slashes, since
 * every `gitsail-cli` path argument in this file is a POSIX-style relative
 * path (matching `--path`'s own examples in `cli.rs`). */
function relativeToRepo(repoRoot: string, absolutePath: string): string {
  return path.relative(repoRoot, absolutePath).split(path.sep).join("/");
}

function renderFileContentResult(content: FileContentDto | undefined, failureMessage: string | undefined): string {
  if (!content) {
    return `(Could not load file content: ${failureMessage ?? "unknown error"})`;
  }
  switch (content.kind) {
    case "text":
      return content.content;
    case "binary":
      return `(binary file — GitSail cannot display it as text, revision ${content.revision})`;
    case "missing":
      return `(this file did not exist at revision ${content.revision})`;
  }
}

// Re-exported so `extension.ts` can build items for a commit-diff file
// picker without importing `historyPresentation.ts` directly for this one
// case (kept here since it depends on `CommitDiffDto`, not a generic DTO
// `historyPresentation.ts` otherwise deals with).
export function buildCommitDiffFileQuickPickItems(commitDiff: CommitDiffDto): QuickPickItemLike[] {
  return commitDiff.diff.files.map((file) => ({
    id: file.path,
    label: file.previousPath ? `${file.previousPath} → ${file.path}` : file.path,
    description: file.changeType,
  }));
}
