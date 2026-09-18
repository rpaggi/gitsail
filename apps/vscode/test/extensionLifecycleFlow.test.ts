// T-255/US-122 criterion 2 ("extensão testa lifecycle/blame/history na
// v0.4"): every existing test exercises `ExtensionController` (activation,
// repository detection, T-201..T-204) or `HistoryController` (blame,
// commit details, file/line history, T-205..T-209) in isolation — each
// `describe` block in `controller.test.ts`/`historyController.test.ts`
// builds its own controller from scratch and never wires the two together
// the way the real extension does. This file closes that gap: one test
// walks the actual runtime sequence `extension.ts::activate()` performs —
// create `ExtensionController`, create `HistoryController`, wire
// `controller.onRepositoryContextChanged` into
// `historyController.onRepositoryContextChanged` — and then drives
// activation -> blame decoration -> opening full commit details -> file
// history, end to end, against a single fake CLI client (no real `vscode`,
// no spawned process — same "testable orchestration" doubles
// `controller.test.ts`/`historyController.test.ts` already establish).

import { beforeEach, describe, expect, it, vi } from "vitest";

import { ExtensionController } from "../src/controller";
import { COMMANDS, HistoryController } from "../src/historyController";
import {
  ConfigurationLike,
  Disposable,
  ExtensionHost,
  OutputChannelLike,
  StatusBarItemLike,
  TextEditorLike,
} from "../src/hostTypes";
import {
  BlameDecorationRenderEntry,
  ConfigurationLike as HistoryConfigurationLike,
  Disposable as HistoryDisposable,
  HistoryDocumentLike,
  HistoryHost,
  HistoryTextEditorLike,
  QuickPickItemLike,
  UriLike,
} from "../src/historyHostTypes";
import { Envelope } from "../src/protocol";

vi.mock("../src/cliLocator", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/cliLocator")>();
  return { ...actual, probeCliBinary: vi.fn() };
});
vi.mock("../src/cliClient", () => ({ GitSailCliClient: vi.fn() }));

import { probeCliBinary } from "../src/cliLocator"; // eslint-disable-line import/order
import { GitSailCliClient } from "../src/cliClient"; // eslint-disable-line import/order

const probeCliBinaryMock = probeCliBinary as unknown as ReturnType<typeof vi.fn>;
const GitSailCliClientMock = GitSailCliClient as unknown as ReturnType<typeof vi.fn>;

const okProbe = {
  status: "ok" as const,
  command: "gitsail",
  source: "path" as const,
  version: { raw: "0.0.0", major: 0, minor: 0, patch: 0 },
};

function envelope<T>(data: T): Envelope<T> {
  return { status: "ok", schemaVersion: 1, requestId: "req-1", data };
}

/** A minimal in-memory `ExtensionHost` double — trimmed to what this
 * lifecycle flow actually needs (activation, one active editor, no
 * multi-root switching), mirroring `controller.test.ts`'s own `FakeHost`. */
class FakeHost implements ExtensionHost {
  workspaceFolders: { uri: { fsPath: string } }[] = [];
  isWorkspaceTrusted = true;
  activeTextEditor: TextEditorLike | undefined;
  readonly statusBarItem: StatusBarItemLike & { visible: boolean } = {
    text: "",
    tooltip: undefined,
    visible: false,
    show() {
      this.visible = true;
    },
    hide() {
      this.visible = false;
    },
    dispose: () => {},
  };
  private readonly editorListeners = new Set<(editor: TextEditorLike | undefined) => void>();

  onDidChangeWorkspaceFolders(): Disposable {
    return { dispose: () => {} };
  }
  onDidGrantWorkspaceTrust(): Disposable {
    return { dispose: () => {} };
  }
  onDidChangeActiveTextEditor(listener: (editor: TextEditorLike | undefined) => void): Disposable {
    this.editorListeners.add(listener);
    return { dispose: () => this.editorListeners.delete(listener) };
  }
  onDidChangeConfiguration(): Disposable {
    return { dispose: () => {} };
  }
  getConfiguration(): ConfigurationLike {
    return { get: <T,>(_key: string, defaultValue: T): T => defaultValue };
  }
  createOutputChannel(): OutputChannelLike {
    return { appendLine: () => {}, dispose: () => {} };
  }
  createStatusBarItem(): StatusBarItemLike {
    return this.statusBarItem;
  }
  showWarningMessage(): void {}
}

/** A minimal in-memory `HistoryHost` double, trimmed the same way — see
 * `historyController.test.ts`'s own `FakeHistoryHost` for the full version
 * every other EPIC-15 test uses. */
class FakeHistoryHost implements HistoryHost {
  activeTextEditor: HistoryTextEditorLike | undefined;
  configValues: Record<string, unknown> = {};

  readonly decorationCalls: { editor: HistoryTextEditorLike; entries: readonly BlameDecorationRenderEntry[] }[] = [];
  readonly documentOpens: string[] = [];
  quickPickAnswer: QuickPickItemLike | undefined;
  showQuickPickOverride: ((items: readonly QuickPickItemLike[]) => Promise<QuickPickItemLike | undefined>) | undefined;

  private readonly selectionListeners = new Set<(editor: HistoryTextEditorLike) => void>();
  private readonly commands = new Map<string, (...args: never[]) => void | Promise<void>>();

  onDidChangeActiveTextEditor(): HistoryDisposable {
    return { dispose: () => {} };
  }
  onDidChangeTextEditorSelection(listener: (editor: HistoryTextEditorLike) => void): HistoryDisposable {
    this.selectionListeners.add(listener);
    return { dispose: () => this.selectionListeners.delete(listener) };
  }
  onDidChangeVisibleRanges(): HistoryDisposable {
    return { dispose: () => {} };
  }
  onDidChangeTextDocument(): HistoryDisposable {
    return { dispose: () => {} };
  }
  onDidChangeConfiguration(): HistoryDisposable {
    return { dispose: () => {} };
  }
  getConfiguration(): HistoryConfigurationLike {
    return {
      get: <T,>(key: string, defaultValue: T): T =>
        key in this.configValues ? (this.configValues[key] as T) : defaultValue,
      update: async <T,>(key: string, value: T): Promise<void> => {
        this.configValues[key] = value;
      },
    };
  }
  setBlameDecorations(editor: HistoryTextEditorLike, entries: readonly BlameDecorationRenderEntry[]): void {
    this.decorationCalls.push({ editor, entries });
  }
  registerCommand(id: string, handler: (...args: never[]) => void | Promise<void>): HistoryDisposable {
    this.commands.set(id, handler);
    return { dispose: () => this.commands.delete(id) };
  }
  async showQuickPick(items: readonly QuickPickItemLike[]): Promise<QuickPickItemLike | undefined> {
    if (this.showQuickPickOverride) {
      return this.showQuickPickOverride(items);
    }
    return this.quickPickAnswer;
  }
  async showInformationMessage(): Promise<string | undefined> {
    return undefined;
  }
  async showWarningMessage(): Promise<string | undefined> {
    return undefined;
  }
  showErrorMessage(): void {}
  async writeClipboardText(): Promise<void> {}
  async openDiff(): Promise<void> {}
  async openDocument(uri: string): Promise<void> {
    this.documentOpens.push(uri);
  }
  registerHistoryContentProvider(): HistoryDisposable {
    return { dispose: () => {} };
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
}

function editorFor(fsPath: string): TextEditorLike {
  return { document: { uri: { scheme: "file", fsPath }, isDirty: false, isUntitled: false } };
}

function historyEditorFor(fsPath: string): HistoryTextEditorLike {
  return {
    document: { uri: { scheme: "file", fsPath }, isDirty: false, version: 1 },
    activeLine: 1,
    selection: { startLine: 1, endLine: 1 },
    visibleRanges: [{ startLine: 1, endLine: 5 }],
  };
}

describe("Extension lifecycle: activation -> blame -> commit details -> file history (T-255/US-122)", () => {
  let runMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers();
    runMock = vi.fn();
    GitSailCliClientMock.mockImplementation(function (this: unknown) {
      return { run: runMock };
    });
    probeCliBinaryMock.mockReset();
    probeCliBinaryMock.mockResolvedValue(okProbe);
  });

  it("wires ExtensionController into HistoryController exactly like extension.ts, and a single fake CLI client answers the whole chain", async () => {
    const repoRoot = "/workspace/project";
    const filePath = `${repoRoot}/src/main.rs`;
    const commitHash = "a".repeat(40);

    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: repoRoot } }];
    host.activeTextEditor = editorFor(filePath);

    const historyHost = new FakeHistoryHost();

    runMock.mockImplementation(async (args: readonly string[]) => {
      switch (args[0]) {
        case "open":
          return envelope({
            id: repoRoot,
            rootPath: repoRoot,
            worktreePath: repoRoot,
            isBare: false,
            headState: { state: "attached", branch: "main" },
            currentBranch: "main",
          });
        case "blame":
          return envelope({
            file: "src/main.rs",
            revision: null,
            lines: [
              {
                finalLine: 1,
                originalLine: 1,
                commit: commitHash,
                author: { name: "Ada", email: "ada@example.com" },
                timestamp: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
                content: "fn main() {}",
                origin: "committed" as const,
              },
            ],
          });
        case "log":
          return envelope({
            items: [
              {
                hash: commitHash,
                shortHash: commitHash.slice(0, 8),
                parents: [],
                author: { name: "Ada", email: "ada@example.com" },
                committer: { name: "Ada", email: "ada@example.com" },
                authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
                commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
                subject: "Initial commit",
                body: "",
                decorations: [],
                isMerge: false,
                isRoot: true,
              },
            ],
            hasMore: false,
          });
        default:
          // The commit-subject cache (for blame hovers) and the commit
          // details lookup both fall through here, exactly like the
          // equivalent fallback in `historyController.test.ts`.
          return envelope({ hash: args[args.length - 1], subject: "Initial commit" });
      }
    });

    // -- 1. Activation: exactly `extension.ts::activate()`'s own sequence.
    const controller = new ExtensionController(host);
    const historyController = new HistoryController(historyHost);
    historyController.activate();
    controller.onRepositoryContextChanged((client, repoRootArg) =>
      historyController.onRepositoryContextChanged(client, repoRootArg),
    );

    await controller.activate();

    expect(host.statusBarItem.visible).toBe(true);
    expect(host.statusBarItem.text).toContain("main");
    expect(runMock).toHaveBeenCalledWith(["open", "--repo", repoRoot]);

    // -- 2. Blame: the active editor's line 1 gets decorated from the
    // repository context `ExtensionController` just resolved and handed
    // over — never a repository this controller detected on its own.
    historyHost.configValues = { "blame.delayMs": 0 };
    historyHost.changeSelection(historyEditorFor(filePath));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(0);

    const decorated = historyHost.decorationCalls.at(-1);
    expect(decorated).toBeDefined();
    expect(decorated!.entries).toHaveLength(1);
    expect(decorated!.entries[0].hoverMarkdown).toContain("Initial commit");

    // -- 3. Commit details: opening the full details for the blamed commit
    // re-queries the CLI through the same bound client (never a decoration
    // string reused as if it were the full commit).
    await historyHost.triggerCommand(COMMANDS.openCommitDetails, { hash: commitHash });
    expect(historyHost.documentOpens).toHaveLength(1);
    expect(historyHost.documentOpens[0]).toContain("gitsail-commit:");
    expect(historyHost.documentOpens[0]).toContain(commitHash);

    // -- 4. File history: browsing the same file's history lists the same
    // commit, reached through the identical bound client/repo root.
    let shownItems: QuickPickItemLike[] = [];
    historyHost.showQuickPickOverride = async (items) => {
      shownItems = [...items];
      return undefined;
    };
    await historyHost.triggerCommand(COMMANDS.showFileHistory);

    expect(shownItems).toHaveLength(1);
    expect(shownItems[0].id).toBe(commitHash);
    expect(shownItems[0].label).toContain("Initial commit");
  });
});
