import { beforeEach, describe, expect, it, vi } from "vitest";

import { ExtensionController } from "../src/controller";
import {
  ConfigurationLike,
  Disposable,
  ExtensionHost,
  OutputChannelLike,
  StatusBarItemLike,
  TextEditorLike,
} from "../src/hostTypes";
// `ExtensionController` orchestrates Git availability and repository
// resolution. Both are covered against a real `git` elsewhere
// (`gitProcess.test.ts`, `gitClient.test.ts`, `repositoryContext.test.ts`);
// here they are mocked so these tests are about the *orchestration*
// (T-201/T-204 lifecycle, caching, notification throttling),
// deterministically and without spawning anything.
//
// ADR-025 note: the "binary" this controller used to locate is gone, so
// what is mocked here is `probeGit` (can we run git at all) and `GitClient`
// (the seven queries), rather than `probeCliBinary`/`GitSailCliClient`.
vi.mock("../src/git/gitClient", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/git/gitClient")>();
  return { ...actual, probeGit: vi.fn(), GitClient: vi.fn() };
});

import { GitClient, probeGit } from "../src/git/gitClient"; // eslint-disable-line import/order
import type { DiscoveryOutcome } from "../src/git/gitClient"; // eslint-disable-line import/order

const probeGitMock = probeGit as unknown as ReturnType<typeof vi.fn>;
const GitClientMock = GitClient as unknown as ReturnType<typeof vi.fn>;

const okProbe = { status: "ok" as const, version: "2.43.0" };

function repositoryOutcome(rootPath: string, branch: string): DiscoveryOutcome {
  return {
    kind: "repository",
    repository: {
      id: rootPath,
      rootPath,
      worktreePath: rootPath,
      isBare: false,
      headState: { state: "attached", branch },
      currentBranch: branch,
    },
  };
}

const noRepositoryOutcome: DiscoveryOutcome = { kind: "no-repository" };

/** A minimal, fully in-memory `ExtensionHost` double with test helpers to
 * fire each event this controller subscribes to (T-201/T-204's DoD:
 * "activation, active folder switching, external file", "untrusted
 * workspace, dirty buffer, disposal during an in-flight query"). */
class FakeHost implements ExtensionHost {
  workspaceFolders: { uri: { fsPath: string } }[] = [];
  isWorkspaceTrusted = true;
  activeTextEditor: TextEditorLike | undefined;

  readonly outputLines: string[] = [];
  readonly warnings: string[] = [];
  readonly statusBarItem: StatusBarItemLike & { visible: boolean; disposeCalls: number } = {
    text: "",
    tooltip: undefined,
    visible: false,
    disposeCalls: 0,
    show() {
      this.visible = true;
    },
    hide() {
      this.visible = false;
    },
    dispose() {
      this.disposeCalls++;
    },
  };
  outputChannelDisposeCalls = 0;
  configValues: Record<string, string> = {};

  private readonly editorListeners = new Set<(editor: TextEditorLike | undefined) => void>();
  private readonly folderListeners = new Set<() => void>();
  private readonly trustListeners = new Set<() => void>();
  private readonly configListeners = new Set<(section: string) => void>();
  readonly disposeCallsByKind = { editor: 0, folder: 0, trust: 0, config: 0 };

  onDidChangeActiveTextEditor(listener: (editor: TextEditorLike | undefined) => void): Disposable {
    this.editorListeners.add(listener);
    return { dispose: () => { this.editorListeners.delete(listener); this.disposeCallsByKind.editor++; } };
  }
  onDidChangeWorkspaceFolders(listener: () => void): Disposable {
    this.folderListeners.add(listener);
    return { dispose: () => { this.folderListeners.delete(listener); this.disposeCallsByKind.folder++; } };
  }
  onDidGrantWorkspaceTrust(listener: () => void): Disposable {
    this.trustListeners.add(listener);
    return { dispose: () => { this.trustListeners.delete(listener); this.disposeCallsByKind.trust++; } };
  }
  onDidChangeConfiguration(listener: (section: string) => void): Disposable {
    this.configListeners.add(listener);
    return { dispose: () => { this.configListeners.delete(listener); this.disposeCallsByKind.config++; } };
  }

  getConfiguration(_section: string): ConfigurationLike {
    return { get: <T,>(key: string, defaultValue: T): T => (this.configValues[key] as T) ?? defaultValue };
  }
  createOutputChannel(_name: string): OutputChannelLike {
    return { appendLine: (v: string) => this.outputLines.push(v), dispose: () => { this.outputChannelDisposeCalls++; } };
  }
  createStatusBarItem(): StatusBarItemLike {
    return this.statusBarItem;
  }
  showWarningMessage(message: string): void {
    this.warnings.push(message);
  }

  // -- test helpers -------------------------------------------------------
  changeActiveEditor(editor: TextEditorLike | undefined): void {
    this.activeTextEditor = editor;
    for (const l of this.editorListeners) l(editor);
  }
  changeWorkspaceFolders(folders: { uri: { fsPath: string } }[]): void {
    this.workspaceFolders = folders;
    for (const l of this.folderListeners) l();
  }
  grantTrust(): void {
    this.isWorkspaceTrusted = true;
    for (const l of this.trustListeners) l();
  }
  changeConfig(): void {
    for (const l of this.configListeners) l("gitsail");
  }
}

function editorFor(path: string, opts: Partial<{ isDirty: boolean; isUntitled: boolean }> = {}): TextEditorLike {
  return {
    document: {
      uri: { scheme: "file", fsPath: path },
      isDirty: opts.isDirty ?? false,
      isUntitled: opts.isUntitled ?? false,
    },
  };
}

describe("ExtensionController", () => {
  let discoverMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    discoverMock = vi.fn();
    // A plain function (not an arrow function) so `new GitClient(...)`
    // works: returning an object from a constructor call overrides `this`,
    // which arrow functions — never constructible — cannot do.
    GitClientMock.mockImplementation(function () {
      return { discover: discoverMock };
    });
    probeGitMock.mockReset();
    probeGitMock.mockResolvedValue(okProbe);
  });

  it("activation resolves the initial active editor's repository and shows it in the status bar", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/src/main.rs");
    discoverMock.mockResolvedValue(repositoryOutcome("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.statusBarItem.visible).toBe(true);
    expect(host.statusBarItem.text).toContain("main");
    expect(discoverMock).toHaveBeenCalledWith("/workspace/project");
  });

  it("switching the active folder in a multi-root workspace re-resolves against the newly active folder", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [
      { uri: { fsPath: "/workspace/a" } },
      { uri: { fsPath: "/workspace/b" } },
    ];
    host.activeTextEditor = editorFor("/workspace/a/file.ts");
    discoverMock.mockImplementation(async (root: string) =>
      repositoryOutcome(root, root === "/workspace/a" ? "branch-a" : "branch-b"),
    );

    const controller = new ExtensionController(host);
    await controller.activate();
    expect(host.statusBarItem.text).toContain("branch-a");

    host.changeActiveEditor(editorFor("/workspace/b/file.ts"));
    await vi.waitFor(() => expect(host.statusBarItem.text).toContain("branch-b"));
  });

  it("a file without a repository hides the status bar without any warning, and never re-queries the same folder", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/plain-folder" } }];
    host.activeTextEditor = editorFor("/workspace/plain-folder/notes.txt");
    discoverMock.mockResolvedValue(noRepositoryOutcome);

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.statusBarItem.visible).toBe(false);
    expect(host.warnings).toHaveLength(0);

    // Switching to a second file under the SAME folder must hit the cache,
    // not re-run discovery (US-068 criterion 3).
    host.changeActiveEditor(editorFor("/workspace/plain-folder/other.txt"));
    await vi.waitFor(() => expect(discoverMock).toHaveBeenCalledTimes(1));
    expect(host.warnings).toHaveLength(0);
  });

  it("a file outside every workspace folder ('external file') still resolves without error", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/elsewhere/other-repo/file.rs");
    discoverMock.mockResolvedValue(repositoryOutcome("/elsewhere/other-repo", "feature"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(discoverMock).toHaveBeenCalledWith("/elsewhere/other-repo");
    expect(host.statusBarItem.text).toContain("feature");
  });

  it("no active editor at all resolves to no-context without ever querying git", async () => {
    const host = new FakeHost();
    host.activeTextEditor = undefined;

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(discoverMock).not.toHaveBeenCalled();
    expect(host.statusBarItem.visible).toBe(false);
  });

  it("an untrusted workspace runs no git query at all, and warns exactly once across repeated editor switches", async () => {
    // ADR-025 strengthened this gate. It used to block only a configured
    // `gitsail.binaryPath` (PATH discovery still ran); with that setting
    // gone, the remaining workspace-controlled input is the repository
    // itself, and running git honors that repository's own configuration —
    // so nothing runs until the workspace is trusted, which is what
    // package.json's `untrustedWorkspaces` description already promised.
    const host = new FakeHost();
    host.isWorkspaceTrusted = false;
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/a.ts");

    const controller = new ExtensionController(host);
    await controller.activate();
    expect(host.warnings).toHaveLength(1);
    expect(host.warnings[0]).toMatch(/not trusted/i);
    expect(probeGitMock).not.toHaveBeenCalled();
    expect(discoverMock).not.toHaveBeenCalled();

    host.changeActiveEditor(editorFor("/workspace/project/b.ts"));
    await vi.waitFor(() => expect(host.outputLines.length).toBeGreaterThan(1));
    expect(host.warnings).toHaveLength(1); // still just the one toast
    expect(discoverMock).not.toHaveBeenCalled(); // still nothing ran

    // Granting trust re-probes and allows a fresh notification if git
    // still cannot be used.
    probeGitMock.mockResolvedValue({ status: "not-found" });
    host.grantTrust();
    await vi.waitFor(() => expect(host.warnings).toHaveLength(2));
    expect(host.warnings[1]).toMatch(/install git/i);
  });

  it("a trusted workspace where git is missing warns once with actionable guidance", async () => {
    probeGitMock.mockResolvedValue({ status: "not-found" });
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/a.ts");

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.warnings).toHaveLength(1);
    expect(host.warnings[0]).toMatch(/install git/i);
    expect(host.warnings[0]).toMatch(/PATH/);
    expect(host.statusBarItem.visible).toBe(false);
    expect(discoverMock).not.toHaveBeenCalled();
  });

  it("a dirty (unsaved) buffer logs an explicit disk-vs-buffer note", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/dirty.ts", { isDirty: true });
    discoverMock.mockResolvedValue(repositoryOutcome("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.outputLines.some((line) => /unsaved/i.test(line) && /disk/i.test(line))).toBe(true);
  });

  it("disposing while a query is in flight discards its result instead of updating a disposed status bar", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/main.rs");

    let resolveRun!: (value: DiscoveryOutcome) => void;
    discoverMock.mockImplementation(
      () =>
        new Promise<DiscoveryOutcome>((resolve) => {
          resolveRun = resolve;
        }),
    );

    const controller = new ExtensionController(host);
    const activatePromise = controller.activate();

    await vi.waitFor(() => expect(discoverMock).toHaveBeenCalled());
    controller.dispose();
    resolveRun(repositoryOutcome("/workspace/project", "late-branch"));

    await activatePromise;

    expect(host.statusBarItem.visible).toBe(false);
    expect(host.statusBarItem.text).toBe("");
    expect(host.statusBarItem.disposeCalls).toBe(1);
    expect(host.outputChannelDisposeCalls).toBe(1);
  });

  it("dispose() removes every subscription it registered", async () => {
    const host = new FakeHost();
    host.activeTextEditor = undefined;
    const controller = new ExtensionController(host);
    await controller.activate();

    controller.dispose();

    expect(host.disposeCallsByKind).toEqual({ editor: 1, folder: 1, trust: 1, config: 1 });
  });

  it("switching the active editor after disposal is a no-op (no leaked listener callback)", async () => {
    const host = new FakeHost();
    host.activeTextEditor = undefined;
    const controller = new ExtensionController(host);
    await controller.activate();
    controller.dispose();

    // The controller unsubscribed on dispose, so this must not even reach
    // its listener — asserting on the host's own bookkeeping proves that,
    // independently of the controller's internals.
    expect(() => host.changeActiveEditor(editorFor("/workspace/anything.ts"))).not.toThrow();
    expect(discoverMock).not.toHaveBeenCalled();
  });
});

describe("ExtensionController.onRepositoryContextChanged (EPIC-15's hook into repository binding)", () => {
  let discoverMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    discoverMock = vi.fn();
    GitClientMock.mockImplementation(function () {
      return { discover: discoverMock };
    });
    probeGitMock.mockReset();
    probeGitMock.mockResolvedValue(okProbe);
  });

  it("fires immediately with the current binding for a listener registered after activation", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/src/main.rs");
    discoverMock.mockResolvedValue(repositoryOutcome("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    const calls: { client: unknown; repoRoot: string | undefined }[] = [];
    controller.onRepositoryContextChanged((client, repoRoot) => calls.push({ client, repoRoot }));

    expect(calls).toEqual([{ client: expect.anything(), repoRoot: "/workspace/project" }]);
  });

  it("notifies undefined/undefined when the active file has no repository", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/plain-folder" } }];
    host.activeTextEditor = editorFor("/workspace/plain-folder/notes.txt");
    discoverMock.mockResolvedValue(noRepositoryOutcome);

    const controller = new ExtensionController(host);
    await controller.activate();

    const calls: { client: unknown; repoRoot: string | undefined }[] = [];
    controller.onRepositoryContextChanged((client, repoRoot) => calls.push({ client, repoRoot }));

    expect(calls).toEqual([{ client: undefined, repoRoot: undefined }]);
  });

  it("notifies again when switching to a different repository across a multi-root workspace", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [
      { uri: { fsPath: "/workspace/a" } },
      { uri: { fsPath: "/workspace/b" } },
    ];
    host.activeTextEditor = editorFor("/workspace/a/file.ts");
    discoverMock.mockImplementation(async (root: string) =>
      repositoryOutcome(root, root === "/workspace/a" ? "branch-a" : "branch-b"),
    );

    const controller = new ExtensionController(host);
    await controller.activate();

    const roots: (string | undefined)[] = [];
    controller.onRepositoryContextChanged((_client, repoRoot) => roots.push(repoRoot));
    expect(roots).toEqual(["/workspace/a"]);

    host.changeActiveEditor(editorFor("/workspace/b/file.ts"));
    await vi.waitFor(() => expect(roots).toEqual(["/workspace/a", "/workspace/b"]));
  });

  it("notifies undefined on dispose, so a listener never retains a stale binding", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/main.rs");
    discoverMock.mockResolvedValue(repositoryOutcome("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    const roots: (string | undefined)[] = [];
    controller.onRepositoryContextChanged((_client, repoRoot) => roots.push(repoRoot));
    controller.dispose();

    expect(roots).toEqual(["/workspace/project", undefined]);
  });
});
