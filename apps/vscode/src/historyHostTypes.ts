// The `vscode`-facing surface EPIC-15 (blame decorations, hover, file/line
// history, commit diff, Desktop handoff) needs, expressed as our own
// interface — the same pattern `hostTypes.ts` established for EPIC-14
// (T-201..T-204): every EPIC-15 business rule (`historyController.ts`) is
// written against `HistoryHost`, a plain object a test can implement
// directly, and only `extension.ts` ever adapts the real `vscode` module to
// it. `hostTypes.ts` itself is left untouched (T-201..T-204 already
// shipped) — this is a sibling interface for the epic that owns decorations
// /hover/commands (SAD §16), not a replacement.
//
// Line numbers everywhere in this file are 1-based, inclusive — matching
// `gitsail`'s own `--range START-END` and every DTO in `dto.ts`
// (`BlameLineDto.finalLine`, `LineRangeDto`, ...). `extension.ts` converts
// to/from VS Code's 0-based `Position`/`Range` exactly once, at the
// adapter boundary — nothing past that boundary ever reasons about 0- vs.
// 1-based line numbers again.

export interface Disposable {
  dispose(): void;
}

export interface HistoryDocumentLike {
  uri: { scheme: string; fsPath: string };
  isDirty: boolean;
  /** VS Code's own per-edit version counter — used as the "content
   * version" key for `BlameQueryCache` (never re-derived by hashing
   * content ourselves; VS Code already maintains this). */
  version: number;
}

export interface LineRangeLike {
  startLine: number;
  endLine: number;
}

export interface HistoryTextEditorLike {
  document: HistoryDocumentLike;
  activeLine: number;
  selection: LineRangeLike;
  visibleRanges: readonly LineRangeLike[];
  /** An opaque handle back to the real `vscode.TextEditor` this was adapted
   * from — `historyController.ts` never reads it, it only ever passes a
   * `HistoryTextEditorLike` it received (e.g. via `setBlameDecorations`)
   * back out unchanged; only `extension.ts`'s adapter casts it back to a
   * real editor to call `editor.setDecorations(...)`. Left `undefined` in
   * every test double, since test doubles never need it. */
  readonly raw?: unknown;
}

export interface BlameDecorationRenderEntry {
  line: number;
  contentText: string;
  /** Already-sanitized Markdown (see `hoverSanitizer.ts`) — this host
   * layer never re-sanitizes, it only renders. */
  hoverMarkdown: string;
  /** Command ids the hover's own links reference, for a scoped
   * `MarkdownString.isTrusted.enabledCommands` (T-206 criterion 3). */
  hoverEnabledCommands: readonly string[];
}

export interface QuickPickItemLike {
  id: string;
  label: string;
  description?: string;
  detail?: string;
}

export interface ConfigurationLike {
  get<T>(key: string, defaultValue: T): T;
  /** Persists a setting change (US-072 criterion 2: blame "pode ser
   * desligado" — the toggle command must actually flip the setting, not
   * just clear decorations for the current session). */
  update<T>(key: string, value: T): Promise<void>;
}

/** A parsed `gitsail-history:` URI, structurally identical to a real
 * `vscode.Uri` (`scheme`/`path`/`query`) — see `historyUri.ts`. */
export interface UriLike {
  scheme: string;
  path: string;
  query: string;
}

export interface HistoryHost {
  readonly activeTextEditor: HistoryTextEditorLike | undefined;

  onDidChangeActiveTextEditor(listener: (editor: HistoryTextEditorLike | undefined) => void): Disposable;
  onDidChangeTextEditorSelection(listener: (editor: HistoryTextEditorLike) => void): Disposable;
  onDidChangeVisibleRanges(listener: (editor: HistoryTextEditorLike) => void): Disposable;
  onDidChangeTextDocument(listener: (document: HistoryDocumentLike) => void): Disposable;
  /** Fires when a `gitsail.*` setting changes by any means (Settings UI,
   * `settings.json` edit, or this extension's own `toggleBlame` command) —
   * `historyController.ts` reacts the same way regardless of which one
   * caused it (US-072 criterion 2). */
  onDidChangeConfiguration(listener: () => void): Disposable;

  getConfiguration(section: string): ConfigurationLike;

  /** Replaces the full set of blame decorations for `editor` in one call
   * (never incremental patches) — the simplest way to guarantee a
   * decoration for a line that is no longer in the current plan (e.g. after
   * an edit shifts lines around) cannot linger (T-205 DoD: "nenhuma
   * decoração de linha errada fica 'grudada'"). An empty `entries` array
   * clears every decoration. */
  setBlameDecorations(editor: HistoryTextEditorLike, entries: readonly BlameDecorationRenderEntry[]): void;

  registerCommand(id: string, handler: (...args: never[]) => void | Promise<void>): Disposable;

  showQuickPick(
    items: readonly QuickPickItemLike[],
    placeholder: string,
  ): Promise<QuickPickItemLike | undefined>;
  showInformationMessage(message: string, ...actions: string[]): Promise<string | undefined>;
  showWarningMessage(message: string, ...actions: string[]): Promise<string | undefined>;
  showErrorMessage(message: string): void;

  writeClipboardText(text: string): Promise<void>;

  /** Opens VS Code's native diff editor comparing two documents (T-209
   * criterion 3) — both `leftUri`/`rightUri` are full URI strings (either
   * `gitsail-history:` history URIs or, in principle, any scheme VS Code
   * can already open). */
  openDiff(leftUri: string, rightUri: string, title: string): Promise<void>;

  /** Opens a single URI as a read-only-by-scheme document (T-206 criterion
   * 2's commit-details panel — no diff, just one document shown). */
  openDocument(uri: string): Promise<void>;

  /** Registers the `TextDocumentContentProvider` backing both
   * `gitsail-history:` (T-209 criterion 3) and `gitsail-commit:` (T-206
   * criterion 2) URIs — one resolver handles both schemes, dispatching on
   * `uri.scheme` itself (see `historyController.ts::resolveHistoryContent`)
   * — `resolveContent` is handed the parsed `UriLike` (never a raw string)
   * and returns the document's full text; it never throws — every
   * "cannot render this" outcome (binary content, missing path, a CLI
   * failure) resolves to a one-line explanatory string instead. */
  registerHistoryContentProvider(resolveContent: (uri: UriLike) => Promise<string>): Disposable;
}
