import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useUpdateStore } from "./update";

describe("update store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("has no outcome and is not checking before check() is ever called", () => {
    const store = useUpdateStore();

    expect(store.outcome).toBeNull();
    expect(store.isChecking).toBe(false);
    expect(store.hasUpdate).toBe(false);
  });

  it("check(automatic) forwards the trigger and records an update-available outcome", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "check_for_update") {
        receivedArgs = args;
        return {
          state: "updateAvailable",
          currentTag: "v0.4.0",
          release: {
            tag: "v0.5.0",
            htmlUrl: "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0",
            checksumsUrl:
              "https://github.com/rpaggi/gitsail/releases/download/v0.5.0/SHA256SUMS.txt",
          },
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useUpdateStore();

    await store.check("automatic");

    expect(receivedArgs).toEqual({ trigger: "automatic" });
    expect(store.hasUpdate).toBe(true);
    expect(store.isChecking).toBe(false);
    expect(store.lastError).toBeNull();
  });

  it("a checkFailed outcome (offline/malformed) is recorded as data, never thrown as an error", async () => {
    mockIPC((cmd) => {
      if (cmd === "check_for_update") {
        return {
          state: "checkFailed",
          error: { code: "network_failure", message: "could not check for updates: offline" },
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useUpdateStore();

    await store.check("manual");

    expect(store.outcome?.state).toBe("checkFailed");
    expect(store.hasUpdate).toBe(false);
    expect(store.lastError).toBeNull();
  });

  it("a skipped outcome (disabled/throttled) never sets hasUpdate", async () => {
    mockIPC((cmd) => {
      if (cmd === "check_for_update") {
        return { state: "skipped", reason: { kind: "disabled" } };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useUpdateStore();

    await store.check("automatic");

    expect(store.outcome?.state).toBe("skipped");
    expect(store.hasUpdate).toBe(false);
  });

  it("a transport failure reaching the command itself is recorded in lastError without throwing", async () => {
    mockIPC(() => {
      throw { code: "internal", message: "ipc boom" };
    });
    const store = useUpdateStore();

    await store.check("manual");

    expect(store.lastError?.message).toBe("ipc boom");
    expect(store.isChecking).toBe(false);
  });

  it("openLink forwards the url to the backend", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "open_update_link") {
        receivedArgs = args;
        return true;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = useUpdateStore();

    await store.openLink("https://github.com/rpaggi/gitsail/releases/tag/v0.5.0");

    expect(receivedArgs).toEqual({ url: "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0" });
  });
});
