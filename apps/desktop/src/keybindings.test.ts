import { describe, expect, it } from "vitest";

import {
  bindingFromKeyboardEvent,
  CONFIGURABLE_ACTIONS,
  effectiveBindings,
  findBindingConflicts,
  formatBindingForDisplay,
  isMacPlatform,
  matchesBinding,
  type KeyEventLike,
} from "./keybindings";

function keyEvent(overrides: Partial<KeyEventLike> = {}): KeyEventLike {
  return { ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, key: "k", ...overrides };
}

describe("bindingFromKeyboardEvent", () => {
  it("builds Mod+<KEY> when Ctrl is held", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, key: "k" }))).toBe("Mod+K");
  });

  it("treats Meta (Cmd) the same as Ctrl", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ metaKey: true, key: "k" }))).toBe("Mod+K");
  });

  it("adds Shift and Alt in a stable order", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, shiftKey: true, key: "f" }))).toBe(
      "Mod+Shift+F",
    );
    expect(
      bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, shiftKey: true, altKey: true, key: "f" })),
    ).toBe("Mod+Shift+Alt+F");
  });

  it("normalizes a single printable character regardless of shifted case", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, key: "k" }))).toBe(
      bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, key: "K" })),
    );
  });

  it("keeps a multi-character key name as-is (Enter, ArrowUp, ...)", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, key: "Enter" }))).toBe("Mod+Enter");
  });

  it("refuses a binding with no Mod held (scope decision: never breaks plain typing)", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ shiftKey: true, key: "k" }))).toBeNull();
    expect(bindingFromKeyboardEvent(keyEvent({ key: "k" }))).toBeNull();
  });

  it("refuses a bare modifier key even when it reports itself as held", () => {
    expect(bindingFromKeyboardEvent(keyEvent({ ctrlKey: true, key: "Control" }))).toBeNull();
    expect(bindingFromKeyboardEvent(keyEvent({ metaKey: true, key: "Meta" }))).toBeNull();
  });
});

describe("matchesBinding", () => {
  it("matches an event against its own canonical binding", () => {
    const event = keyEvent({ ctrlKey: true, shiftKey: true, key: "f" });
    expect(matchesBinding(event, "Mod+Shift+F")).toBe(true);
  });

  it("does not match a different binding", () => {
    const event = keyEvent({ ctrlKey: true, key: "k" });
    expect(matchesBinding(event, "Mod+Shift+F")).toBe(false);
  });
});

describe("effectiveBindings", () => {
  it("uses each action's default when no override is set", () => {
    const bindings = effectiveBindings(CONFIGURABLE_ACTIONS, {});
    for (const action of CONFIGURABLE_ACTIONS) {
      expect(bindings[action.id]).toBe(action.defaultBinding);
    }
  });

  it("prefers a persisted override over the default", () => {
    const bindings = effectiveBindings(CONFIGURABLE_ACTIONS, { "focus-search": "Mod+Shift+K" });
    expect(bindings["focus-search"]).toBe("Mod+Shift+K");
  });

  it("never invents bindings for actions outside the registry", () => {
    const bindings = effectiveBindings(CONFIGURABLE_ACTIONS, { "made-up-action": "Mod+Z" });
    expect(bindings["made-up-action"]).toBeUndefined();
  });
});

describe("findBindingConflicts", () => {
  it("reports no conflicts when every binding is unique", () => {
    const conflicts = findBindingConflicts({ a: "Mod+K", b: "Mod+L" });
    expect(conflicts.size).toBe(0);
  });

  it("groups every action id sharing a binding", () => {
    const conflicts = findBindingConflicts({ a: "Mod+K", b: "Mod+K", c: "Mod+L" });
    expect(conflicts.get("Mod+K")).toEqual(["a", "b"]);
    expect(conflicts.has("Mod+L")).toBe(false);
  });

  it("supports a three-way conflict on the same binding", () => {
    const conflicts = findBindingConflicts({ a: "Mod+K", b: "Mod+K", c: "Mod+K" });
    expect(conflicts.get("Mod+K")).toEqual(["a", "b", "c"]);
  });
});

describe("formatBindingForDisplay", () => {
  it("renders Mod as Ctrl off macOS", () => {
    expect(formatBindingForDisplay("Mod+Shift+F", false)).toBe("Ctrl+Shift+F");
  });

  it("renders Mod as Cmd on macOS", () => {
    expect(formatBindingForDisplay("Mod+Shift+F", true)).toBe("Cmd+Shift+F");
  });
});

describe("isMacPlatform", () => {
  it("detects macOS from the platform string", () => {
    expect(isMacPlatform({ platform: "MacIntel", userAgent: "" })).toBe(true);
  });

  it("detects non-macOS platforms", () => {
    expect(isMacPlatform({ platform: "Win32", userAgent: "" })).toBe(false);
  });

  it("falls back to false when neither field is available", () => {
    expect(isMacPlatform(undefined)).toBe(false);
  });
});
