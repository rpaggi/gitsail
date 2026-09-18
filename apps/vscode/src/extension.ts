// The extension's actual entry point — the *only* file in this package
// allowed to `import "vscode"` (see `hostTypes.ts`'s doc comment for why
// everything else is built against that interface instead). Kept
// deliberately thin: this only adapts the real `vscode` namespace to
// `ExtensionHost` and hands it to `ExtensionController`, which owns every
// actual decision this extension makes.
//
// Activation event: `onStartupFinished` (see `package.json`) rather than
// something like `workspaceContains:**/.git`. The latter would mean the
// extension host itself pattern-matches for a Git repository before ever
// starting — exactly the "`.git` detection on the extension side" US-068
// criterion 1 rules out, even as a coarse activation gate. Activating
// unconditionally after startup and then asking `gitsail open` (which may
// answer "no repository") keeps *all* repository detection on the Core
// side, with no exception for the activation path.

import * as vscode from "vscode";

import { ExtensionController } from "./controller";
import { COMMIT_DETAILS_URI_SCHEME } from "./commitDetailsUri";
import { HistoryController } from "./historyController";
import {
  BlameDecorationRenderEntry,
  ConfigurationLike as HistoryConfigurationLike,
  Disposable as HistoryDisposable,
  HistoryDocumentLike,
  HistoryHost,
  HistoryTextEditorLike,
  QuickPickItemLike,
  UriLike,
} from "./historyHostTypes";
import { HISTORY_URI_SCHEME } from "./historyUri";
import {
  ConfigurationLike,
  Disposable,
  ExtensionHost,
  OutputChannelLike,
  StatusBarItemLike,
  TextEditorLike,
} from "./hostTypes";

function adaptTextEditor(editor: vscode.TextEditor | undefined): TextEditorLike | undefined {
  if (!editor) {
    return undefined;
  }
  const document = editor.document;
  return {
    document: {
      uri: { scheme: document.uri.scheme, fsPath: document.uri.fsPath },
      isDirty: document.isDirty,
      isUntitled: document.isUntitled,
    },
  };
}

function createHost(): ExtensionHost {
  return {
    get workspaceFolders() {
      return (vscode.workspace.workspaceFolders ?? []).map((folder) => ({
        uri: { fsPath: folder.uri.fsPath },
      }));
    },
    get isWorkspaceTrusted() {
      return vscode.workspace.isTrusted;
    },
    get activeTextEditor() {
      return adaptTextEditor(vscode.window.activeTextEditor);
    },

    onDidChangeWorkspaceFolders(listener: () => void): Disposable {
      return vscode.workspace.onDidChangeWorkspaceFolders(listener);
    },
    onDidGrantWorkspaceTrust(listener: () => void): Disposable {
      return vscode.workspace.onDidGrantWorkspaceTrust(listener);
    },
    onDidChangeActiveTextEditor(listener: (editor: TextEditorLike | undefined) => void): Disposable {
      return vscode.window.onDidChangeActiveTextEditor((editor) => listener(adaptTextEditor(editor)));
    },
    onDidChangeConfiguration(listener: (section: string) => void): Disposable {
      return vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("gitsail")) {
          listener("gitsail");
        }
      });
    },

    getConfiguration(section: string): ConfigurationLike {
      const configuration = vscode.workspace.getConfiguration(section);
      return {
        get<T>(key: string, defaultValue: T): T {
          return configuration.get<T>(key, defaultValue);
        },
      };
    },
    createOutputChannel(name: string): OutputChannelLike {
      return vscode.window.createOutputChannel(name);
    },
    createStatusBarItem(): StatusBarItemLike {
      const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
      // Adapted explicitly rather than returned as-is: the real
      // `StatusBarItem.tooltip` also accepts a `MarkdownString`, which
      // `StatusBarItemLike` deliberately does not widen to, since nothing
      // in this extension needs Markdown tooltips (yet).
      return {
        get text() {
          return item.text;
        },
        set text(value: string) {
          item.text = value;
        },
        get tooltip() {
          return typeof item.tooltip === "string" ? item.tooltip : undefined;
        },
        set tooltip(value: string | undefined) {
          item.tooltip = value;
        },
        show: () => item.show(),
        hide: () => item.hide(),
        dispose: () => item.dispose(),
      };
    },
    showWarningMessage(message: string): void {
      void vscode.window.showWarningMessage(message);
    },
  };
}

// ---------------------------------------------------------------------
// EPIC-15 (blame decorations, hover, file/line history, commit diff,
// Desktop handoff) — adapts the real `vscode` module to `HistoryHost`
// (`historyHostTypes.ts`), the same "one adapter, all logic elsewhere"
// pattern as `createHost()`/`ExtensionHost` above. All line numbers are
// converted between VS Code's 0-based and this extension's 1-based
// convention exactly here, nowhere else (see `historyHostTypes.ts`'s doc
// comment).
// ---------------------------------------------------------------------

function toLineRange(range: vscode.Range): { startLine: number; endLine: number } {
  return { startLine: range.start.line + 1, endLine: range.end.line + 1 };
}

function adaptHistoryTextEditor(editor: vscode.TextEditor | undefined): HistoryTextEditorLike | undefined {
  if (!editor) {
    return undefined;
  }
  const document = editor.document;
  return {
    document: {
      uri: { scheme: document.uri.scheme, fsPath: document.uri.fsPath },
      isDirty: document.isDirty,
      version: document.version,
    },
    activeLine: editor.selection.active.line + 1,
    selection: toLineRange(editor.selection),
    visibleRanges: editor.visibleRanges.map(toLineRange),
    raw: editor,
  };
}

function adaptHistoryDocument(document: vscode.TextDocument): HistoryDocumentLike {
  return {
    uri: { scheme: document.uri.scheme, fsPath: document.uri.fsPath },
    isDirty: document.isDirty,
    version: document.version,
  };
}

/** The one decoration type every blame decoration reuses (creating a new
 * `TextEditorDecorationType` per render would leak one every time). Its
 * options are set per-call via `DecorationOptions.renderOptions`, so the
 * type itself carries no fixed styling beyond what VS Code always shows for
 * an "after" content-text decoration. */
function createBlameDecorationType(): vscode.TextEditorDecorationType {
  return vscode.window.createTextEditorDecorationType({
    after: {
      margin: "0 0 0 1.5em",
      color: new vscode.ThemeColor("editorCodeLens.foreground"),
    },
  });
}

function buildBlameDecorationOptions(entries: readonly BlameDecorationRenderEntry[]): vscode.DecorationOptions[] {
  return entries.map((entry) => {
    const line = Math.max(0, entry.line - 1);
    const hover = new vscode.MarkdownString(entry.hoverMarkdown);
    // Scoped trust (T-206 criterion 3): only the specific command ids this
    // hover's own content declared (via `HoverContentBuilder.addCommandLink`)
    // may ever execute — never a blanket `true`, and never `false` either,
    // since that would also disable this extension's own legitimate
    // "open full commit details" link.
    hover.isTrusted =
      entry.hoverEnabledCommands.length > 0 ? { enabledCommands: [...entry.hoverEnabledCommands] } : false;
    return {
      range: new vscode.Range(line, 0, line, 0),
      renderOptions: { after: { contentText: `  ${entry.contentText}` } },
      hoverMessage: hover,
    };
  });
}

function createHistoryHost(): HistoryHost {
  const decorationType = createBlameDecorationType();
  return {
    get activeTextEditor() {
      return adaptHistoryTextEditor(vscode.window.activeTextEditor);
    },

    onDidChangeActiveTextEditor(listener): HistoryDisposable {
      return vscode.window.onDidChangeActiveTextEditor((editor) => listener(adaptHistoryTextEditor(editor)));
    },
    onDidChangeTextEditorSelection(listener): HistoryDisposable {
      return vscode.window.onDidChangeTextEditorSelection((event) => {
        if (event.textEditor === vscode.window.activeTextEditor) {
          const adapted = adaptHistoryTextEditor(event.textEditor);
          if (adapted) {
            listener(adapted);
          }
        }
      });
    },
    onDidChangeVisibleRanges(listener): HistoryDisposable {
      return vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
        if (event.textEditor === vscode.window.activeTextEditor) {
          const adapted = adaptHistoryTextEditor(event.textEditor);
          if (adapted) {
            listener(adapted);
          }
        }
      });
    },
    onDidChangeTextDocument(listener): HistoryDisposable {
      return vscode.workspace.onDidChangeTextDocument((event) => listener(adaptHistoryDocument(event.document)));
    },
    onDidChangeConfiguration(listener): HistoryDisposable {
      return vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("gitsail")) {
          listener();
        }
      });
    },

    getConfiguration(section: string): HistoryConfigurationLike {
      const configuration = vscode.workspace.getConfiguration(section);
      return {
        get<T>(key: string, defaultValue: T): T {
          return configuration.get<T>(key, defaultValue);
        },
        async update<T>(key: string, value: T): Promise<void> {
          await configuration.update(key, value, vscode.ConfigurationTarget.Global);
        },
      };
    },

    setBlameDecorations(editor, entries): void {
      const realEditor = editor.raw as vscode.TextEditor | undefined;
      if (realEditor && vscode.window.visibleTextEditors.includes(realEditor)) {
        realEditor.setDecorations(decorationType, buildBlameDecorationOptions(entries));
      }
    },

    registerCommand(id, handler): HistoryDisposable {
      return vscode.commands.registerCommand(id, handler);
    },

    async showQuickPick(items: readonly QuickPickItemLike[], placeholder: string) {
      // `items` already structurally satisfies `vscode.QuickPickItem`
      // (`label`/`description?`/`detail?`), plus this extension's own `id`
      // — VS Code returns the exact same object reference the caller
      // picked, so `id` survives the round trip untouched.
      return vscode.window.showQuickPick(items as (QuickPickItemLike & vscode.QuickPickItem)[], {
        placeHolder: placeholder,
      });
    },
    async showInformationMessage(message: string, ...actions: string[]) {
      return vscode.window.showInformationMessage(message, ...actions);
    },
    async showWarningMessage(message: string, ...actions: string[]) {
      return vscode.window.showWarningMessage(message, ...actions);
    },
    showErrorMessage(message: string): void {
      void vscode.window.showErrorMessage(message);
    },

    async writeClipboardText(text: string): Promise<void> {
      await vscode.env.clipboard.writeText(text);
    },

    async openDiff(leftUri: string, rightUri: string, title: string): Promise<void> {
      await vscode.commands.executeCommand(
        "vscode.diff",
        vscode.Uri.parse(leftUri),
        vscode.Uri.parse(rightUri),
        title,
      );
    },
    async openDocument(uri: string): Promise<void> {
      const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(uri));
      await vscode.window.showTextDocument(document, { preview: false });
    },

    registerHistoryContentProvider(resolveContent: (uri: UriLike) => Promise<string>): HistoryDisposable {
      const provider: vscode.TextDocumentContentProvider = {
        provideTextDocumentContent: (uri) =>
          resolveContent({ scheme: uri.scheme, path: uri.path, query: uri.query }),
      };
      // One resolver, registered for both read-only content schemes this
      // extension defines (`gitsail-history:` for a file's content at a
      // revision, `gitsail-commit:` for a commit's full details) — the
      // resolver itself dispatches on `uri.scheme` (see
      // `HistoryController.resolveHistoryContent`).
      const historyRegistration = vscode.workspace.registerTextDocumentContentProvider(
        HISTORY_URI_SCHEME,
        provider,
      );
      const commitRegistration = vscode.workspace.registerTextDocumentContentProvider(
        COMMIT_DETAILS_URI_SCHEME,
        provider,
      );
      return {
        dispose: () => {
          historyRegistration.dispose();
          commitRegistration.dispose();
        },
      };
    },
  };
}

let controller: ExtensionController | undefined;
let historyController: HistoryController | undefined;

export function activate(_context: vscode.ExtensionContext): void {
  controller = new ExtensionController(createHost());
  void controller.activate();

  historyController = new HistoryController(createHistoryHost());
  historyController.activate();
  controller.onRepositoryContextChanged((client, repoRoot) =>
    historyController?.onRepositoryContextChanged(client, repoRoot),
  );
}

export function deactivate(): void {
  historyController?.dispose();
  historyController = undefined;
  controller?.dispose();
  controller = undefined;
}
