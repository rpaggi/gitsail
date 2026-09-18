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

let controller: ExtensionController | undefined;

export function activate(_context: vscode.ExtensionContext): void {
  controller = new ExtensionController(createHost());
  void controller.activate();
}

export function deactivate(): void {
  controller?.dispose();
  controller = undefined;
}
