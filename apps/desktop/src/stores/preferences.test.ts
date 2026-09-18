import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { usePreferencesStore } from "./preferences";

describe("preferences store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.documentElement.removeAttribute("data-theme");
  });

  afterEach(() => {
    clearMocks();
  });

  it("defaults to dark even before load() resolves (US-106 criterion 1)", () => {
    const store = usePreferencesStore();

    expect(store.effectiveTheme).toBe("dark");
  });

  it("load applies the persisted theme and marks the document root", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_preferences") return { theme: "light" };
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePreferencesStore();

    await store.load();

    expect(store.theme).toBe("light");
    expect(store.effectiveTheme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(store.diagnostic).toBeNull();
  });

  it("load surfaces a recovered-from-corruption diagnostic without treating it as a hard error", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_preferences") {
        return { theme: "dark", diagnostic: { code: "parse_failure", message: "corrupted" } };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePreferencesStore();

    await store.load();

    expect(store.lastError).toBeNull();
    expect(store.diagnostic?.message).toBe("corrupted");
  });

  it("load records a failure without throwing, and still applies the dark default", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "boom" };
    });
    const store = usePreferencesStore();

    await store.load();

    expect(store.lastError?.message).toBe("boom");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("setTheme applies immediately and persists", async () => {
    const received: unknown[] = [];
    mockIPC((cmd, args) => {
      received.push([cmd, args]);
      return { theme: "light" };
    });
    const store = usePreferencesStore();

    await store.setTheme("light");

    expect(store.effectiveTheme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(received).toEqual([["set_theme", { theme: "light" }]]);
  });

  it("setTheme keeps the chosen theme applied even when persistence fails", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "disk full" };
    });
    const store = usePreferencesStore();

    await store.setTheme("light");

    expect(store.effectiveTheme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(store.lastError?.message).toBe("disk full");
  });

  it("defaults to check-for-updates on even before load() resolves (T-260/US-127)", () => {
    const store = usePreferencesStore();

    expect(store.checkForUpdates).toBe(true);
  });

  it("load applies the persisted check-for-updates preference", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_preferences") return { theme: "dark", checkForUpdates: false };
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePreferencesStore();

    await store.load();

    expect(store.checkForUpdates).toBe(false);
  });

  it("setCheckForUpdates applies immediately and persists", async () => {
    const received: unknown[] = [];
    mockIPC((cmd, args) => {
      received.push([cmd, args]);
      return { theme: "dark", checkForUpdates: false };
    });
    const store = usePreferencesStore();

    await store.setCheckForUpdates(false);

    expect(store.checkForUpdates).toBe(false);
    expect(received).toEqual([["set_check_for_updates", { enabled: false }]]);
  });

  it("setCheckForUpdates keeps the chosen value applied even when persistence fails", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "disk full" };
    });
    const store = usePreferencesStore();

    await store.setCheckForUpdates(false);

    expect(store.checkForUpdates).toBe(false);
    expect(store.lastError?.message).toBe("disk full");
  });
});
