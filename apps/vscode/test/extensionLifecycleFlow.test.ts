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
// history, end to end, against a single fake git client (no real `vscode`,
// no spawned process — same "testable orchestration" doubles
// `controller.test.ts`/`historyController.test.ts` already establish).
//
// ADR-025 note: the double is now a `GitClient` with typed methods rather
// than a CLI client returning JSON envelopes. What this test proves is
// unchanged — that `extension.ts`'s wiring hands one resolved client and
// repository root from `ExtensionController` to `HistoryController`, and
// that the whole chain runs off that single binding.

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
import type { BlameDto, CommitDto, PageDto } from "../src/dto";

vi.mock("../src/git/gitClient", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/git/gitClient")>();
  return { ...actual, probeGit: vi.fn(), GitClient: vi.fn() };
});

import { GitClient, probeGit } from "../src/git/gitClient"; // eslint-disable-line import/order
import type { DiscoveryOutcome } from "../src/git/gitClient"; // eslint-disable-line import/order

const probeGitMock = probeGit as unknown as ReturnType<typeof vi.fn>;
const GitClientMock = GitClient as unknown as ReturnType<typeof vi.fn>;

const okProbe = { status: "ok" as const, version: "2.43.0" };

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
  let clientStub: Record<string, ReturnType<typeof vi.fn>>;
  let discoverMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers();
    clientStub = {};
    discoverMock = vi.fn();
    GitClientMock.mockImplementation(function (this: unknown) {
      return clientStub;
    });
    probeGitMock.mockReset();
    probeGitMock.mockResolvedValue(okProbe);
  });

  it("wires ExtensionController into HistoryController exactly like extension.ts, and a single fake git client answers the whole chain", async () => {
    const repoRoot = "/workspace/project";
    const filePath = `${repoRoot}/src/main.rs`;
    const commitHash = "a".repeat(40);

    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: repoRoot } }];
    host.activeTextEditor = editorFor(filePath);

    const historyHost = new FakeHistoryHost();

    const commit: CommitDto = {
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
    };
    const blame: BlameDto = {
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
          origin: "committed",
        },
      ],
    };
    const historyPage: PageDto<CommitDto> = { items: [commit], hasMore: false };

    discoverMock.mockResolvedValue({
      kind: "repository",
      repository: {
        id: repoRoot,
        rootPath: repoRoot,
        worktreePath: repoRoot,
        isBare: false,
        headState: { state: "attached", branch: "main" },
        currentBranch: "main",
      },
    } satisfies DiscoveryOutcome);

    // One stub object, shared by ExtensionController and HistoryController
    // — that sharing is the thing this test exists to prove.
    clientStub.discover = discoverMock;
    clientStub.getBlame = vi.fn(async () => blame);
    clientStub.getCommit = vi.fn(async () => commit);
    clientStub.getFileHistoryPage = vi.fn(async () => historyPage);

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
    expect(discoverMock).toHaveBeenCalledWith(repoRoot);

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
    // re-queries git through the same bound client (never a decoration
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
