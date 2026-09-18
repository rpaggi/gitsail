import { afterEach, describe, expect, it, vi } from "vitest";

import { copyToClipboard } from "./clipboard";

describe("clipboard service", () => {
  afterEach(() => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  });

  it("returns true and writes the text when the clipboard API succeeds", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    const result = await copyToClipboard("hello");

    expect(result).toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("returns false without throwing when navigator.clipboard is unavailable", async () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });

    const result = await copyToClipboard("hello");

    expect(result).toBe(false);
  });

  it("returns false without throwing when writeText rejects", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn(async () => {
          throw new Error("denied");
        }),
      },
    });

    const result = await copyToClipboard("hello");

    expect(result).toBe(false);
  });
});
