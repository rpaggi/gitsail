import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useKeybindingsStore } from "./keybindings";
import { CONFIGURABLE_ACTIONS } from "../keybindings";

describe("keybindings store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("uses every action's default binding before load() resolves", () => {
    const store = useKeybindingsStore();

    for (const action of CONFIGURABLE_ACTIONS) {
      expect(store.bindings[action.id]).toBe(action.defaultBinding);
    }
  });

  it("load merges persisted overrides with the defaults", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_keybinding_overrides") return { "focus-search": "Mod+Shift+K" };
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useKeybindingsStore();

    await store.load();

    expect(store.bindings["focus-search"]).toBe("Mod+Shift+K");
    expect(store.bindings.fetch).toBe("Mod+Shift+F");
    expect(store.lastError).toBeNull();
  });

  it("load records a failure without throwing, falling back to defaults", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "boom" };
    });
    const store = useKeybindingsStore();

    await store.load();

    expect(store.lastError?.message).toBe("boom");
    expect(store.bindings["focus-search"]).toBe("Mod+K");
  });

  it("setBinding persists the remap and updates the effective binding", async () => {
    const received: unknown[] = [];
    mockIPC((cmd, args) => {
      received.push([cmd, args]);
      if (cmd === "set_keybinding_override") return { "focus-search": "Mod+Shift+K" };
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useKeybindingsStore();

    await store.setBinding("focus-search", "Mod+Shift+K");

    expect(store.bindings["focus-search"]).toBe("Mod+Shift+K");
    expect(received).toEqual([
      ["set_keybinding_override", { actionId: "focus-search", binding: "Mod+Shift+K" }],
    ]);
  });

  it("resetBinding restores an action's default", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_keybinding_override") return { "focus-search": "Mod+Shift+K" };
      if (cmd === "reset_keybinding_override") return {};
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useKeybindingsStore();
    await store.setBinding("focus-search", "Mod+Shift+K");

    await store.resetBinding("focus-search");

    expect(store.bindings["focus-search"]).toBe("Mod+K");
  });

  it("resetAll clears every override", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_keybinding_override") return { "focus-search": "Mod+Shift+K", fetch: "Mod+Shift+F" };
      if (cmd === "reset_all_keybinding_overrides") return null;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useKeybindingsStore();
    await store.setBinding("focus-search", "Mod+Shift+K");

    await store.resetAll();

    for (const action of CONFIGURABLE_ACTIONS) {
      expect(store.bindings[action.id]).toBe(action.defaultBinding);
    }
  });

  it("conflicts is empty by default (every default binding is unique)", () => {
    const store = useKeybindingsStore();

    expect(store.conflicts.size).toBe(0);
  });

  it("conflicts reports two actions remapped to the same binding", async () => {
    mockIPC((cmd, args) => {
      const a = args as { actionId: string; binding: string };
      if (cmd === "set_keybinding_override") {
        return a.actionId === "pull" ? { pull: "Mod+Shift+F" } : { "focus-search": "Mod+Shift+F" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useKeybindingsStore();

    await store.setBinding("pull", "Mod+Shift+F");
    // Simulate both overrides being persisted (the real backend accumulates
    // them; this double only echoes the latest call, so set the merged
    // state directly for this assertion).
    store.overrides = { pull: "Mod+Shift+F", fetch: "Mod+Shift+F" };

    const conflict = store.conflicts.get("Mod+Shift+F");
    expect(conflict).toEqual(expect.arrayContaining(["pull", "fetch"]));
  });
});
