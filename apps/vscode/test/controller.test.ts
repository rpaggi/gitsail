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
import { Envelope } from "../src/protocol";

// `ExtensionController` orchestrates `cliLocator`/`cliClient` — both are
// already covered by their own contract tests against real spawned
// processes (`cliLocator.test.ts`, `cliClient.test.ts`). Here they are
// mocked so these tests are about the *orchestration* (T-201/T-204
// lifecycle, caching, notification throttling), deterministically and
// without spawning anything.
vi.mock("../src/cliLocator", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../src/cliLocator")>();
  return { ...actual, probeCliBinary: vi.fn() };
});
vi.mock("../src/cliClient", () => ({ GitSailCliClient: vi.fn() }));

import { probeCliBinary } from "../src/cliLocator"; // eslint-disable-line import/order
import { GitSailCliClient } from "../src/cliClient"; // eslint-disable-line import/order

const probeCliBinaryMock = probeCliBinary as unknown as ReturnType<typeof vi.fn>;
const GitSailCliClientMock = GitSailCliClient as unknown as ReturnType<typeof vi.fn>;

const okProbe = { status: "ok" as const, command: "gitsail", source: "path" as const, version: { raw: "0.0.0", major: 0, minor: 0, patch: 0 } };

function repositoryEnvelope(rootPath: string, branch: string): Envelope<unknown> {
  return {
    status: "ok",
    schemaVersion: 1,
    requestId: "req-1",
    data: {
      id: rootPath,
      rootPath,
      worktreePath: rootPath,
      isBare: false,
      headState: { state: "attached", branch },
      currentBranch: branch,
    },
  };
}

const notFoundEnvelope: Envelope<unknown> = {
  status: "error",
  schemaVersion: 1,
  requestId: "req-1",
  error: { code: "repository_not_found", message: "not a Git repository" },
};

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
  configValues: Record<string, string> = { binaryPath: "" };

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
  let runMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    runMock = vi.fn();
    // A plain function (not an arrow function) so `new GitSailCliClient(...)`
    // works: returning an object from a constructor call overrides `this`,
    // which arrow functions — never constructible — cannot do.
    GitSailCliClientMock.mockImplementation(function () {
      return { run: runMock };
    });
    probeCliBinaryMock.mockReset();
    probeCliBinaryMock.mockResolvedValue(okProbe);
  });

  it("activation resolves the initial active editor's repository and shows it in the status bar", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/src/main.rs");
    runMock.mockResolvedValue(repositoryEnvelope("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.statusBarItem.visible).toBe(true);
    expect(host.statusBarItem.text).toContain("main");
    expect(runMock).toHaveBeenCalledWith(["open", "--repo", "/workspace/project"]);
  });

  it("switching the active folder in a multi-root workspace re-resolves against the newly active folder", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [
      { uri: { fsPath: "/workspace/a" } },
      { uri: { fsPath: "/workspace/b" } },
    ];
    host.activeTextEditor = editorFor("/workspace/a/file.ts");
    runMock.mockImplementation(async (args: readonly string[]) => {
      const root = args[2];
      return repositoryEnvelope(root as string, root === "/workspace/a" ? "branch-a" : "branch-b");
    });

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
    runMock.mockResolvedValue(notFoundEnvelope);

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.statusBarItem.visible).toBe(false);
    expect(host.warnings).toHaveLength(0);

    // Switching to a second file under the SAME folder must hit the cache,
    // not the CLI again (US-068 criterion 3).
    host.changeActiveEditor(editorFor("/workspace/plain-folder/other.txt"));
    await vi.waitFor(() => expect(runMock).toHaveBeenCalledTimes(1));
    expect(host.warnings).toHaveLength(0);
  });

  it("a file outside every workspace folder ('external file') still resolves without error", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/elsewhere/other-repo/file.rs");
    runMock.mockResolvedValue(repositoryEnvelope("/elsewhere/other-repo", "feature"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(runMock).toHaveBeenCalledWith(["open", "--repo", "/elsewhere/other-repo"]);
    expect(host.statusBarItem.text).toContain("feature");
  });

  it("no active editor at all resolves to no-context without ever calling the CLI", async () => {
    const host = new FakeHost();
    host.activeTextEditor = undefined;

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(runMock).not.toHaveBeenCalled();
    expect(host.statusBarItem.visible).toBe(false);
  });

  it("an untrusted workspace with a configured binaryPath is blocked and warns exactly once across repeated editor switches", async () => {
    probeCliBinaryMock.mockResolvedValue({ status: "blocked-untrusted" });
    const host = new FakeHost();
    host.isWorkspaceTrusted = false;
    host.configValues.binaryPath = "/opt/gitsail/gitsail";
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/a.ts");

    const controller = new ExtensionController(host);
    await controller.activate();
    expect(host.warnings).toHaveLength(1);

    host.changeActiveEditor(editorFor("/workspace/project/b.ts"));
    await vi.waitFor(() => expect(host.outputLines.length).toBeGreaterThan(1));
    expect(host.warnings).toHaveLength(1); // still just the one toast
    expect(probeCliBinaryMock).toHaveBeenCalledTimes(1); // never re-probed for a plain editor switch

    // Granting trust re-probes and allows a fresh notification if the
    // (now-trusted) binary still cannot be used.
    probeCliBinaryMock.mockResolvedValue({ status: "not-found", command: "/opt/gitsail/gitsail" });
    host.grantTrust();
    await vi.waitFor(() => expect(host.warnings).toHaveLength(2));
  });

  it("a dirty (unsaved) buffer logs an explicit disk-vs-buffer note", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/dirty.ts", { isDirty: true });
    runMock.mockResolvedValue(repositoryEnvelope("/workspace/project", "main"));

    const controller = new ExtensionController(host);
    await controller.activate();

    expect(host.outputLines.some((line) => /unsaved/i.test(line) && /disk/i.test(line))).toBe(true);
  });

  it("disposing while a query is in flight discards its result instead of updating a disposed status bar", async () => {
    const host = new FakeHost();
    host.workspaceFolders = [{ uri: { fsPath: "/workspace/project" } }];
    host.activeTextEditor = editorFor("/workspace/project/main.rs");

    let resolveRun!: (value: Envelope<unknown>) => void;
    runMock.mockImplementation(
      () =>
        new Promise<Envelope<unknown>>((resolve) => {
          resolveRun = resolve;
        }),
    );

    const controller = new ExtensionController(host);
    const activatePromise = controller.activate();

    await vi.waitFor(() => expect(runMock).toHaveBeenCalled());
    controller.dispose();
    resolveRun(repositoryEnvelope("/workspace/project", "late-branch"));

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
    expect(runMock).not.toHaveBeenCalled();
  });
});
