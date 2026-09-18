import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useDiffStore } from "./diff";
import type { DiffDto } from "../services/dto";

function sampleDiff(marker: string): DiffDto {
  return {
    files: [
      {
        path: marker,
        previousPath: null,
        changeType: "modified",
        isBinary: false,
        truncated: false,
        hunks: [],
      },
    ],
  };
}

/** A promise this test can resolve on its own schedule. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe("diff store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("load populates file/staged/diff", async () => {
    mockIPC(() => sampleDiff("a.txt"));
    const store = useDiffStore();

    await store.load("a.txt", true);

    expect(store.file).toBe("a.txt");
    expect(store.staged).toBe(true);
    expect(store.diff?.files[0].path).toBe("a.txt");
    expect(store.lastError).toBeNull();
  });

  it("toggleMode never touches file/staged/diff (criterion 1)", async () => {
    mockIPC(() => sampleDiff("a.txt"));
    const store = useDiffStore();
    await store.load("a.txt", true);

    store.toggleMode();

    expect(store.mode).toBe("sideBySide");
    expect(store.file).toBe("a.txt");
    expect(store.staged).toBe(true);
    expect(store.diff?.files[0].path).toBe("a.txt");

    store.toggleMode();
    expect(store.mode).toBe("unified");
  });

  it("an older in-flight load never overwrites a newer one's result", async () => {
    const slow = deferred<DiffDto>();
    const fast = deferred<DiffDto>();
    let call = 0;
    mockIPC(() => {
      call += 1;
      return call === 1 ? slow.promise : fast.promise;
    });
    const store = useDiffStore();

    const firstLoad = store.load("old.txt", false);
    const secondLoad = store.load("new.txt", false);

    // The second (newer) load resolves first...
    fast.resolve(sampleDiff("new.txt"));
    await secondLoad;
    expect(store.file).toBe("new.txt");

    // ...then the first (now-stale) one finally resolves too, but must not
    // clobber what the newer load already applied.
    slow.resolve(sampleDiff("old.txt"));
    await firstLoad;
    expect(store.file).toBe("new.txt");
  });

  it("a failed load reports the error without clearing a previous successful diff", async () => {
    mockIPC(() => sampleDiff("a.txt"));
    const store = useDiffStore();
    await store.load("a.txt", true);

    mockIPC(() => {
      throw { code: "internal", message: "boom" };
    });
    await store.load("b.txt", true);

    expect(store.lastError?.message).toBe("boom");
  });
});
