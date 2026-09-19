import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { COMMANDS, HistoryController } from "../src/historyController";
import {
  BlameDecorationRenderEntry,
  ConfigurationLike,
  Disposable,
  HistoryDocumentLike,
  HistoryHost,
  HistoryTextEditorLike,
  QuickPickItemLike,
  UriLike,
} from "../src/historyHostTypes";
import { GitClient } from "../src/git/gitClient";
import { BlameDto, CommitDiffDto, CommitDto, FileContentDto, LineHistoryDto, PageDto } from "../src/dto";

class FakeHistoryHost implements HistoryHost {
  activeTextEditor: HistoryTextEditorLike | undefined;
  configValues: Record<string, unknown> = {};

  readonly decorationCalls: { editor: HistoryTextEditorLike; entries: readonly BlameDecorationRenderEntry[] }[] = [];
  readonly infoMessages: string[] = [];
  readonly warningMessages: string[] = [];
  readonly errorMessages: string[] = [];
  readonly clipboardWrites: string[] = [];
  readonly diffCalls: { left: string; right: string; title: string }[] = [];
  readonly documentOpens: string[] = [];
  quickPickAnswer: QuickPickItemLike | undefined;
  messageAction: string | undefined;
  private contentResolver: ((uri: UriLike) => Promise<string>) | undefined;

  private editorListeners = new Set<(editor: HistoryTextEditorLike | undefined) => void>();
  private selectionListeners = new Set<(editor: HistoryTextEditorLike) => void>();
  private visibleRangesListeners = new Set<(editor: HistoryTextEditorLike) => void>();
  private documentChangeListeners = new Set<(document: HistoryDocumentLike) => void>();
  private configChangeListeners = new Set<() => void>();
  private commands = new Map<string, (...args: never[]) => void | Promise<void>>();

  onDidChangeActiveTextEditor(listener: (editor: HistoryTextEditorLike | undefined) => void): Disposable {
    this.editorListeners.add(listener);
    return { dispose: () => this.editorListeners.delete(listener) };
  }
  onDidChangeTextEditorSelection(listener: (editor: HistoryTextEditorLike) => void): Disposable {
    this.selectionListeners.add(listener);
    return { dispose: () => this.selectionListeners.delete(listener) };
  }
  onDidChangeVisibleRanges(listener: (editor: HistoryTextEditorLike) => void): Disposable {
    this.visibleRangesListeners.add(listener);
    return { dispose: () => this.visibleRangesListeners.delete(listener) };
  }
  onDidChangeTextDocument(listener: (document: HistoryDocumentLike) => void): Disposable {
    this.documentChangeListeners.add(listener);
    return { dispose: () => this.documentChangeListeners.delete(listener) };
  }
  onDidChangeConfiguration(listener: () => void): Disposable {
    this.configChangeListeners.add(listener);
    return { dispose: () => this.configChangeListeners.delete(listener) };
  }

  getConfiguration(_section: string): ConfigurationLike {
    return {
      get: <T,>(key: string, defaultValue: T): T =>
        key in this.configValues ? (this.configValues[key] as T) : defaultValue,
      update: async <T,>(key: string, value: T): Promise<void> => {
        this.configValues[key] = value;
        for (const l of this.configChangeListeners) l();
      },
    };
  }

  setBlameDecorations(editor: HistoryTextEditorLike, entries: readonly BlameDecorationRenderEntry[]): void {
    this.decorationCalls.push({ editor, entries });
  }

  registerCommand(id: string, handler: (...args: never[]) => void | Promise<void>): Disposable {
    this.commands.set(id, handler);
    return { dispose: () => this.commands.delete(id) };
  }

  async showQuickPick(_items: readonly QuickPickItemLike[]): Promise<QuickPickItemLike | undefined> {
    return this.quickPickAnswer;
  }
  async showInformationMessage(message: string): Promise<string | undefined> {
    this.infoMessages.push(message);
    return this.messageAction;
  }
  async showWarningMessage(message: string): Promise<string | undefined> {
    this.warningMessages.push(message);
    return this.messageAction;
  }
  showErrorMessage(message: string): void {
    this.errorMessages.push(message);
  }

  async writeClipboardText(text: string): Promise<void> {
    this.clipboardWrites.push(text);
  }

  async openDiff(leftUri: string, rightUri: string, title: string): Promise<void> {
    this.diffCalls.push({ left: leftUri, right: rightUri, title });
  }
  async openDocument(uri: string): Promise<void> {
    this.documentOpens.push(uri);
  }

  registerHistoryContentProvider(resolveContent: (uri: UriLike) => Promise<string>): Disposable {
    this.contentResolver = resolveContent;
    return { dispose: () => (this.contentResolver = undefined) };
  }

  // -- test helpers ---------------------------------------------------------
  triggerCommand(id: string, ...args: unknown[]): Promise<void> | void {
    const handler = this.commands.get(id);
    if (!handler) {
      throw new Error(`no command registered for ${id}`);
    }
    return handler(...(args as never[]));
  }

  changeSelection(editor: HistoryTextEditorLike): void {
    this.activeTextEditor = editor;
    for (const l of this.selectionListeners) l(editor);
  }

  resolveContent(uri: UriLike): Promise<string> {
    if (!this.contentResolver) {
      throw new Error("no content provider registered");
    }
    return this.contentResolver(uri);
  }
}

function editorFor(
  fsPath: string,
  opts: Partial<{ isDirty: boolean; version: number; activeLine: number; selection: { startLine: number; endLine: number }; visibleRanges: { startLine: number; endLine: number }[] }> = {},
): HistoryTextEditorLike {
  return {
    document: { uri: { scheme: "file", fsPath }, isDirty: opts.isDirty ?? false, version: opts.version ?? 1 },
    activeLine: opts.activeLine ?? 1,
    selection: opts.selection ?? { startLine: 1, endLine: 1 },
    visibleRanges: opts.visibleRanges ?? [{ startLine: 1, endLine: 5 }],
  };
}

const REPO_ROOT = "/repo";

/** ADR-025: the controller no longer speaks an argv/envelope protocol to a
 * `gitsail` binary — it calls typed `GitClient` methods that resolve to a
 * DTO or throw. A stub is therefore just the subset of those methods a
 * given test's code path actually reaches. */
function makeClient(overrides: Partial<Record<keyof GitClient, unknown>>): GitClient {
  return overrides as unknown as GitClient;
}

/** `CommitDto` is total (twelve required fields); tests only ever care
 * about one or two of them, so the rest get deterministic defaults here
 * rather than being repeated at every call site. */
function commitDto(partial: Partial<CommitDto> = {}): CommitDto {
  return {
    hash: "0".repeat(40),
    shortHash: "00000000",
    parents: [],
    author: { name: "Ada", email: "a@x.com" },
    committer: { name: "Ada", email: "a@x.com" },
    authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
    subject: "subject",
    body: "",
    decorations: [],
    isMerge: false,
    isRoot: false,
    ...partial,
  };
}

describe("HistoryController: blame decorations (T-205/US-072)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("debounces recompute and decorates only the current line by default", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.delayMs": 100 };
    const client = makeClient({
      getBlame: vi.fn(
        async (): Promise<BlameDto> => ({
          file: "a.ts",
          revision: null,
          lines: [
            { finalLine: 1, originalLine: 1, commit: "a".repeat(40), author: { name: "Ada", email: "a@x.com" }, timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 }, content: "x", origin: "committed" },
            { finalLine: 2, originalLine: 2, commit: "b".repeat(40), author: { name: "Bob", email: "b@x.com" }, timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 }, content: "y", origin: "committed" },
          ],
        }),
      ),
      getCommit: vi.fn(async (_repoRoot: string, revision: string) =>
        commitDto({ hash: revision, subject: "A subject" }),
      ),
    });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);

    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`, { activeLine: 1 }));

    await vi.advanceTimersByTimeAsync(100);
    await vi.advanceTimersByTimeAsync(0);

    const lastCall = host.decorationCalls.at(-1);
    expect(lastCall).toBeDefined();
    expect(lastCall!.entries.map((e) => e.line)).toEqual([1]);
  });

  it("all-visible-lines mode decorates every visible line", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.delayMs": 0, "blame.mode": "allVisibleLines" };
    const client = makeClient({
      getBlame: vi.fn(
        async (): Promise<BlameDto> => ({
          file: "a.ts",
          revision: null,
          lines: [1, 2, 3].map((n) => ({
            finalLine: n,
            originalLine: n,
            commit: "a".repeat(40),
            author: { name: "Ada", email: "a@x.com" },
            timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
            content: `line ${n}`,
            origin: "committed" as const,
          })),
        }),
      ),
      getCommit: vi.fn(async (_repoRoot: string, revision: string) =>
        commitDto({ hash: revision, subject: "subject" }),
      ),
    });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`, { visibleRanges: [{ startLine: 1, endLine: 3 }] }));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    const lastCall = host.decorationCalls.at(-1);
    expect(lastCall!.entries.map((e) => e.line).sort()).toEqual([1, 2, 3]);
  });

  it("disabling blame clears decorations instead of running a git query", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.enabled": false };
    const getBlame = vi.fn(async (): Promise<BlameDto> => ({ file: "a.ts", revision: null, lines: [] }));
    const client = makeClient({ getBlame });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`));

    await vi.advanceTimersByTimeAsync(0);

    expect(getBlame).not.toHaveBeenCalled();
    expect(host.decorationCalls.at(-1)?.entries).toEqual([]);
  });

  it("T-255/US-122 criterion 3 (\"repo vazio\"): blaming a file in a repository with no commits yet (unborn HEAD) clears decorations without throwing", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.delayMs": 0 };
    const client = makeClient({
      getBlame: vi
        .fn()
        .mockRejectedValue(new Error("HEAD is unborn — the repository has no commits yet")),
    });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    // A blame failure — an unborn HEAD here, a missing/unusable `git`
    // elsewhere — is never surfaced as a per-line error toast (it would be
    // far too noisy on every cursor move); it silently clears decorations,
    // exactly like the "disabled" case above.
    expect(host.decorationCalls.at(-1)?.entries).toEqual([]);
    expect(host.errorMessages).toEqual([]);
  });

  it("never invents an author for an uncommitted line, and appends a disk-vs-buffer note for a dirty document", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.delayMs": 0 };
    const client = makeClient({
      getBlame: vi.fn(
        async (): Promise<BlameDto> => ({
          file: "a.ts",
          revision: null,
          lines: [
            {
              finalLine: 1,
              originalLine: 1,
              commit: "0".repeat(40),
              author: { name: "Not Committed Yet", email: "not.committed.yet" },
              timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
              content: "x",
              origin: "local",
            },
          ],
        }),
      ),
      getCommit: vi.fn(async () => commitDto()),
    });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`, { isDirty: true, activeLine: 1 }));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    const entry = host.decorationCalls.at(-1)!.entries[0];
    expect(entry.contentText).toBe("Uncommitted change");
    expect(entry.contentText).not.toContain("Not Committed Yet");
    expect(entry.hoverMarkdown).toMatch(/unsaved/i);
  });

  it("a malicious commit author/message never becomes an executable hover link, end to end (T-206 criterion 3)", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.delayMs": 0 };
    const maliciousHash = "c".repeat(40);
    const client = makeClient({
      getBlame: vi.fn(
        async (): Promise<BlameDto> => ({
          file: "a.ts",
          revision: null,
          lines: [
            {
              finalLine: 1,
              originalLine: 1,
              commit: maliciousHash,
              author: { name: "[pwned](command:workbench.action.terminal.new)", email: "a@x.com" },
              timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
              content: "x",
              origin: "committed",
            },
          ],
        }),
      ),
      getCommit: vi.fn(async () =>
        commitDto({
          hash: maliciousHash,
          subject: "[Click here](command:workbench.action.terminal.new)",
        }),
      ),
    });

    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.changeSelection(editorFor(`${REPO_ROOT}/a.ts`, { activeLine: 1 }));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    const entry = host.decorationCalls.at(-1)!.entries[0];
    // The malicious author name/subject can only ever appear escaped —
    // the exact `](command:` adjacency that makes it an active Markdown
    // link must never survive for *their* text (this extension's own two
    // legitimate action links below deliberately keep that exact syntax,
    // so the check must target the attacker's payload specifically, not
    // just search the whole hover for the substring).
    expect(entry.hoverMarkdown).not.toMatch(/pwned]\(command:/);
    expect(entry.hoverMarkdown).not.toMatch(/Click here]\(command:/);
    expect(entry.hoverMarkdown).toContain("\\[pwned\\]\\(command:workbench");
    expect(entry.hoverMarkdown).toContain("\\[Click here\\]\\(command:workbench");
    // Only this extension's own two command ids (attached by
    // `recomputeBlameDecorations` itself) are ever allow-listed — never a
    // command id sourced from repository text.
    expect(new Set(entry.hoverEnabledCommands)).toEqual(
      new Set([COMMANDS.openCommitDetails, COMMANDS.copyCommitHash]),
    );

    // A normal, non-malicious message still renders its real content and
    // still gets the same two legitimate actions.
    const normalHash = "d".repeat(40);
    const normalClient = makeClient({
      getBlame: vi.fn(
        async (): Promise<BlameDto> => ({
          file: "a.ts",
          revision: null,
          lines: [
            {
              finalLine: 1,
              originalLine: 1,
              commit: normalHash,
              author: { name: "Ada Lovelace", email: "ada@example.com" },
              timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
              content: "x",
              origin: "committed",
            },
          ],
        }),
      ),
      getCommit: vi.fn(async () => commitDto({ hash: normalHash, subject: "Fix the pagination cursor" })),
    });
    const normalHost = new FakeHistoryHost();
    normalHost.configValues = { "blame.delayMs": 0 };
    const normalController = new HistoryController(normalHost);
    normalController.activate();
    normalController.onRepositoryContextChanged(normalClient, REPO_ROOT);
    normalHost.changeSelection(editorFor(`${REPO_ROOT}/a.ts`, { activeLine: 1 }));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    const normalEntry = normalHost.decorationCalls.at(-1)!.entries[0];
    expect(normalEntry.hoverMarkdown).toContain("Ada Lovelace");
    expect(normalEntry.hoverMarkdown).toContain("Fix the pagination cursor");
    expect(new Set(normalEntry.hoverEnabledCommands)).toEqual(
      new Set([COMMANDS.openCommitDetails, COMMANDS.copyCommitHash]),
    );
  });
});

describe("HistoryController: toggling blame (US-072 criterion 2)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("persists the flipped gitsail.blame.enabled setting and clears decorations when turned off", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.enabled": true, "blame.delayMs": 0 };
    const client = makeClient({
      getBlame: vi.fn(async (): Promise<BlameDto> => ({ file: "a.ts", revision: null, lines: [] })),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`);

    await host.triggerCommand(COMMANDS.toggleBlame);

    expect(host.configValues["blame.enabled"]).toBe(false);
    expect(host.infoMessages.some((m) => /disabled/i.test(m))).toBe(true);
    // Flipping the setting fires onDidChangeConfiguration, which this
    // controller reacts to by clearing decorations (recompute sees
    // enabled: false) without needing a separate manual clear call.
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);
    expect(host.decorationCalls.at(-1)?.entries).toEqual([]);
  });

  it("toggling back on re-enables the setting", async () => {
    const host = new FakeHistoryHost();
    host.configValues = { "blame.enabled": false };
    const controller = new HistoryController(host);
    controller.activate();

    await host.triggerCommand(COMMANDS.toggleBlame);

    expect(host.configValues["blame.enabled"]).toBe(true);
    expect(host.infoMessages.some((m) => /enabled/i.test(m))).toBe(true);
  });
});

describe("HistoryController: commit details and hash copy (T-206/US-076 criterion 2)", () => {
  it("opens a gitsail-commit: document, re-querying git rather than reusing a decoration string", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({});
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);

    await host.triggerCommand(COMMANDS.openCommitDetails, { hash: "a".repeat(40) });

    expect(host.documentOpens).toHaveLength(1);
    expect(host.documentOpens[0]).toContain("gitsail-commit:");
  });

  it("copies the full (never abbreviated) hash to the clipboard", async () => {
    const host = new FakeHistoryHost();
    const controller = new HistoryController(host);
    controller.activate();

    const fullHash = "a".repeat(40);
    await host.triggerCommand(COMMANDS.copyCommitHash, { hash: fullHash });

    expect(host.clipboardWrites).toEqual([fullHash]);
    expect(host.infoMessages.some((m) => m.includes(fullHash))).toBe(true);
  });
});

describe("HistoryController: file history (T-207/US-074)", () => {
  it("shows an explicit 'no history' state instead of an unexplained empty list", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({
      getFileHistoryPage: vi.fn(
        async (): Promise<PageDto<CommitDto>> => ({ items: [], hasMore: false }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`);
    host.quickPickAnswer = undefined; // no selection needed; we inspect via override below

    // Capture the items shown by overriding showQuickPick for this test.
    let shownItems: QuickPickItemLike[] = [];
    host.showQuickPick = async (items) => {
      shownItems = [...items];
      return undefined;
    };

    await host.triggerCommand(COMMANDS.showFileHistory);

    expect(shownItems).toHaveLength(1);
    expect(shownItems[0].label).toMatch(/no history/i);
  });

  it("selecting a commit opens its diff for that same file", async () => {
    const host = new FakeHistoryHost();
    const commitHash = "a".repeat(40);
    const client = makeClient({
      getFileHistoryPage: vi.fn(
        async (): Promise<PageDto<CommitDto>> => ({
          items: [commitDto({ hash: commitHash, shortHash: "aaaaaaaa" })],
          hasMore: false,
        }),
      ),
      getCommitDiff: vi.fn(
        async (): Promise<CommitDiffDto> => ({
          target: commitHash,
          base: "b".repeat(40),
          diff: { files: [{ path: "a.ts", previousPath: null, changeType: "modified", isBinary: false, truncated: false, hunks: [] }] },
        }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`);
    host.showQuickPick = async (items) => items.find((i) => i.id === commitHash);

    await host.triggerCommand(COMMANDS.showFileHistory);

    expect(host.diffCalls).toHaveLength(1);
    expect(host.diffCalls[0].left).toContain("b".repeat(40));
    expect(host.diffCalls[0].right).toContain(commitHash);
  });

  it("T-255/US-122 criterion 3 (\"falha\"): a failed git query surfaces a clear error message instead of crashing or showing a bare empty list", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({
      getFileHistoryPage: vi.fn().mockRejectedValue(new Error("git log failed unexpectedly")),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`);

    await host.triggerCommand(COMMANDS.showFileHistory);

    expect(host.errorMessages).toHaveLength(1);
    expect(host.errorMessages[0]).toContain("git log failed unexpectedly");
  });

  it("T-255/US-122 criterion 3 (\"cancelamento\"): dismissing the quick pick (Escape) after real history items are shown does nothing — no diff opens, nothing throws", async () => {
    const host = new FakeHistoryHost();
    const commitHash = "a".repeat(40);
    const client = makeClient({
      getFileHistoryPage: vi.fn(
        async (): Promise<PageDto<CommitDto>> => ({
          items: [commitDto({ hash: commitHash, shortHash: "aaaaaaaa" })],
          hasMore: false,
        }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`);
    // A real, non-empty pick list this time (unlike the "no history" test
    // above) — the user presses Escape without choosing anything.
    host.showQuickPick = async () => undefined;

    await expect(host.triggerCommand(COMMANDS.showFileHistory)).resolves.not.toThrow();

    expect(host.diffCalls).toEqual([]);
    expect(host.errorMessages).toEqual([]);
  });
});

describe("HistoryController: line history (T-208/US-075)", () => {
  it("uses the editor's current selection as the queried range", async () => {
    const host = new FakeHistoryHost();
    const getLineHistory = vi.fn(
      async (): Promise<LineHistoryDto> => ({
        file: "a.ts",
        revision: "HEAD",
        range: { start: 5, end: 9 },
        entries: [],
      }),
    );
    const client = makeClient({ getLineHistory });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`, { selection: { startLine: 5, endLine: 9 } });

    await host.triggerCommand(COMMANDS.showLineHistory);

    expect(getLineHistory).toHaveBeenCalledWith({
      repoRoot: REPO_ROOT,
      filePath: "a.ts",
      startLine: 5,
      endLine: 9,
      revision: undefined,
    });
  });

  it("warns about disk-vs-buffer drift for a dirty document, without inventing an attribution", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({
      getLineHistory: vi.fn(
        async (): Promise<LineHistoryDto> => ({
          file: "a.ts",
          revision: "HEAD",
          range: { start: 1, end: 1 },
          entries: [],
        }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.activeTextEditor = editorFor(`${REPO_ROOT}/a.ts`, { isDirty: true });

    await host.triggerCommand(COMMANDS.showLineHistory);

    expect(host.warningMessages.some((m) => /unsaved/i.test(m) && /disk/i.test(m))).toBe(true);
  });
});

describe("HistoryController: commit diff / open diff (T-209/US-076)", () => {
  it("root commit diff opens the target against an empty (root sentinel) left side", async () => {
    const host = new FakeHistoryHost();
    const commitHash = "a".repeat(40);
    const client = makeClient({
      getCommitDiff: vi.fn(
        async (): Promise<CommitDiffDto> => ({
          target: commitHash,
          base: null,
          diff: { files: [{ path: "a.ts", previousPath: null, changeType: "added", isBinary: false, truncated: false, hunks: [] }] },
        }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);

    await host.triggerCommand(COMMANDS.openCommitDiffForFile, { hash: commitHash, filePath: "a.ts" });

    expect(host.diffCalls).toHaveLength(1);
    expect(host.diffCalls[0].title).toMatch(/root commit/i);
    // The resolver for the left (root/empty) URI must resolve to "" without
    // ever running a git query for it.
    const leftUri = new URL(host.diffCalls[0].left);
    const content = await host.resolveContent({ scheme: "gitsail-history", path: leftUri.pathname, query: leftUri.search.slice(1) });
    expect(content).toBe("");
  });

  it("copies the full hash and offers it when GitSail Desktop is not configured", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({});
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);
    host.messageAction = "Copy commit hash";

    const fullHash = "a".repeat(40);
    await host.triggerCommand(COMMANDS.openInDesktop, { hash: fullHash });

    expect(host.warningMessages.some((m) => /gitsail\.desktop\.path/.test(m))).toBe(true);
    expect(host.clipboardWrites).toEqual([fullHash]);
  });
});

describe("HistoryController: content provider dispatch", () => {
  it("resolves gitsail-history: text content from the file at that revision", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({
      getFileContentAtRevision: vi.fn(
        async (): Promise<FileContentDto> => ({ kind: "text", path: "a.ts", revision: "abc", content: "hello" }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);

    const uriString = `gitsail-history:/a.ts?${new URLSearchParams({ repo: REPO_ROOT, path: "a.ts", revision: "abc" })}`;
    const url = new URL(uriString);
    const content = await host.resolveContent({ scheme: "gitsail-history", path: url.pathname, query: url.search.slice(1) });
    expect(content).toBe("hello");
  });

  it("resolves gitsail-commit: content via the commit use case", async () => {
    const host = new FakeHistoryHost();
    const client = makeClient({
      getCommit: vi.fn(async () =>
        commitDto({ hash: "a".repeat(40), shortHash: "aaaaaaaa", subject: "A subject" }),
      ),
    });
    const controller = new HistoryController(host);
    controller.activate();
    controller.onRepositoryContextChanged(client, REPO_ROOT);

    const uriString = `gitsail-commit:/${"a".repeat(40)}.gitsail-commit?${new URLSearchParams({
      repo: REPO_ROOT,
      hash: "a".repeat(40),
    })}`;
    const url = new URL(uriString);
    const content = await host.resolveContent({ scheme: "gitsail-commit", path: url.pathname, query: url.search.slice(1) });
    expect(content).toContain("A subject");
  });
});
