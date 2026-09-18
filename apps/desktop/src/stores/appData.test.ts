import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useAppDataStore } from "./appData";
import { useRepositorySessionStore } from "./session";
import type { RecentRepositoryDto, RepositoryDto, RepositoryStatusDto } from "../services/dto";

function repo(rootPath: string): RepositoryDto {
  return {
    id: rootPath,
    rootPath,
    worktreePath: rootPath,
    isBare: false,
    headState: { state: "attached", branch: "main" },
    currentBranch: "main",
  };
}

function status(): RepositoryStatusDto {
  return {
    branch: "main",
    headState: { state: "attached", branch: "main" },
    files: [],
    isClean: true,
  };
}

describe("app data store (recent repositories, US-052)", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("starts empty and not loading", () => {
    const store = useAppDataStore();

    expect(store.recentRepositories).toEqual([]);
    expect(store.invalidRecents).toEqual({});
    expect(store.isLoadingRecents).toBe(false);
  });

  it("loadRecentRepositories populates the list from the backend", async () => {
    const entries: RecentRepositoryDto[] = [
      { path: "/repo-a", lastOpenedUnixSeconds: 200 },
      { path: "/repo-b", lastOpenedUnixSeconds: 100 },
    ];
    mockIPC(() => entries);

    const store = useAppDataStore();
    await store.loadRecentRepositories();

    expect(store.recentRepositories).toEqual(entries);
    expect(store.isLoadingRecents).toBe(false);
  });

  it("loadRecentRepositories falls back to an empty list on failure, without throwing", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "could not read the recents file" };
    });

    const store = useAppDataStore();
    await expect(store.loadRecentRepositories()).resolves.toBeUndefined();

    expect(store.recentRepositories).toEqual([]);
  });

  it("openRecentRepository opens a valid entry through the shared session and reloads the list", async () => {
    mockIPC((cmd) => {
      if (cmd === "open_repository") return repo("/repo");
      if (cmd === "get_repository_status") return status();
      if (cmd === "list_recent_repositories") return [{ path: "/repo", lastOpenedUnixSeconds: 300 }];
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = useAppDataStore();
    const session = useRepositorySessionStore();
    await store.openRecentRepository("/repo");

    expect(session.repository?.rootPath).toBe("/repo");
    expect(store.recentRepositories).toEqual([{ path: "/repo", lastOpenedUnixSeconds: 300 }]);
    expect(store.invalidRecents["/repo"]).toBeUndefined();
  });

  it(
    "US-052 criterion 2: a moved/inaccessible entry is marked invalid but stays in the list " +
      "until the user explicitly removes it",
    async () => {
      const entries: RecentRepositoryDto[] = [{ path: "/moved-away", lastOpenedUnixSeconds: 100 }];
      mockIPC((cmd) => {
        if (cmd === "list_recent_repositories") return entries;
        if (cmd === "open_repository") {
          throw { code: "repository_not_found", message: "not a Git repository" };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const store = useAppDataStore();
      await store.loadRecentRepositories();
      await store.openRecentRepository("/moved-away");

      expect(
        store.recentRepositories,
        "the entry must never be silently removed just because opening it failed",
      ).toEqual(entries);
      expect(store.invalidRecents["/moved-away"]?.code).toBe("repository_not_found");
    },
  );

  it("forgetRecentRepository removes the entry and clears any invalid mark for it", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_recent_repositories") return [{ path: "/moved-away", lastOpenedUnixSeconds: 1 }];
      if (cmd === "open_repository") throw { code: "repository_not_found", message: "gone" };
      if (cmd === "forget_recent_repository") return [];
      throw new Error(`unexpected command ${cmd}`);
    });

    const store = useAppDataStore();
    await store.loadRecentRepositories();
    await store.openRecentRepository("/moved-away");
    expect(store.invalidRecents["/moved-away"]).toBeDefined();

    await store.forgetRecentRepository("/moved-away");

    expect(store.recentRepositories).toEqual([]);
    expect(store.invalidRecents["/moved-away"]).toBeUndefined();
  });
});
