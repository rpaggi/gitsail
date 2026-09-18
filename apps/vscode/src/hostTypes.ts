// The minimal slice of the real `vscode` API surface `ExtensionController`
// needs, expressed as our own interface rather than `import * as vscode`
// directly. Two reasons:
//
// 1. `vscode` is only resolvable inside a running extension host — there is
//    no npm-runnable package to import it from in a plain unit test process
//    (see the module doc comment in `extension.ts`). Depending on this
//    interface instead means `controller.ts` and everything it drives stays
//    unit-testable with a plain object literal, no `vscode` mock/module
//    replacement machinery required.
// 2. It documents, in one place, exactly how much of the editor's API this
//    extension actually touches — deliberately small, matching SAD §16's
//    "the extension owns: decorations, hover UI, commands, configuration,
//    editor lifecycle" and nothing about Git itself.

export interface Disposable {
  dispose(): void;
}

export interface WorkspaceFolderLike {
  uri: { fsPath: string };
}

export interface DocumentLike {
  uri: { scheme: string; fsPath: string };
  isDirty: boolean;
  isUntitled: boolean;
}

export interface TextEditorLike {
  document: DocumentLike;
}

export interface ConfigurationLike {
  get<T>(key: string, defaultValue: T): T;
}

export interface OutputChannelLike extends Disposable {
  appendLine(value: string): void;
}

export interface StatusBarItemLike extends Disposable {
  text: string;
  tooltip: string | undefined;
  show(): void;
  hide(): void;
}

/**
 * Everything `ExtensionController` needs from the editor host. The real
 * implementation (`extension.ts`) adapts the actual `vscode` namespace to
 * this shape; tests construct a plain object implementing it directly.
 */
export interface ExtensionHost {
  readonly workspaceFolders: readonly WorkspaceFolderLike[];
  readonly isWorkspaceTrusted: boolean;
  readonly activeTextEditor: TextEditorLike | undefined;

  onDidChangeWorkspaceFolders(listener: () => void): Disposable;
  onDidGrantWorkspaceTrust(listener: () => void): Disposable;
  onDidChangeActiveTextEditor(listener: (editor: TextEditorLike | undefined) => void): Disposable;
  /** `section` is the top-level configuration section that changed (e.g.
   * `"gitsail"`), already narrowed by the adapter via
   * `event.affectsConfiguration`, so this interface never leaks the real
   * `vscode.ConfigurationChangeEvent` shape into testable code. */
  onDidChangeConfiguration(listener: (section: string) => void): Disposable;

  getConfiguration(section: string): ConfigurationLike;
  createOutputChannel(name: string): OutputChannelLike;
  createStatusBarItem(): StatusBarItemLike;
  showWarningMessage(message: string): void;
}
