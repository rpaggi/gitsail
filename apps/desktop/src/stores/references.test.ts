import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useReferencesStore } from "./references";
import type { StashDto, TagDto } from "../services/dto";

function tag(name: string): TagDto {
  return { name, target: "a".repeat(40), kind: { kind: "lightweight" } };
}

function stash(index: number): StashDto {
  return {
    index,
    commit: "a".repeat(40),
    message: "WIP",
    date: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
  };
}

describe("references store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loadTags populates the tag list and clears any previous error", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_tags") return [tag("v1.0"), tag("v2.0")];
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useReferencesStore();

    await store.loadTags();

    expect(store.tags).toHaveLength(2);
    expect(store.tagsError).toBeNull();
    expect(store.isLoadingTags).toBe(false);
  });

  it("loadTags records a failure without throwing, and never fabricates tags", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "boom" };
    });
    const store = useReferencesStore();

    await store.loadTags();

    expect(store.tags).toEqual([]);
    expect(store.tagsError?.message).toBe("boom");
  });

  it("an empty tag list is a legitimate, error-free state", async () => {
    mockIPC(() => []);
    const store = useReferencesStore();

    await store.loadTags();

    expect(store.tags).toEqual([]);
    expect(store.tagsError).toBeNull();
  });

  it("loadStashes populates the stash list and clears any previous error", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_stash_entries") return [stash(0), stash(1)];
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useReferencesStore();

    await store.loadStashes();

    expect(store.stashes).toHaveLength(2);
    expect(store.stashesError).toBeNull();
  });

  it("loadStashes records a failure without throwing", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "stash read failed" };
    });
    const store = useReferencesStore();

    await store.loadStashes();

    expect(store.stashes).toEqual([]);
    expect(store.stashesError?.message).toBe("stash read failed");
  });

  it("loadAll loads tags and stashes independently — one failing never blocks the other", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_tags") throw { code: "internal", message: "tags failed" };
      if (cmd === "list_stash_entries") return [stash(0)];
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useReferencesStore();

    await store.loadAll();

    expect(store.tagsError?.message).toBe("tags failed");
    expect(store.stashes).toHaveLength(1);
    expect(store.stashesError).toBeNull();
  });
});
