import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import {
  checkForUpdate,
  getPreferences,
  openUpdateLink,
  setCheckForUpdates,
  setTheme,
} from "./preferences";
import type { PreferencesDto, UpdateCheckOutcomeDto } from "./dto";

describe("preferences service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("getPreferences invokes get_preferences and returns the DTO as-is", async () => {
    let receivedCommand = "";
    const preferences: PreferencesDto = { theme: "dark", checkForUpdates: true };
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return preferences;
    });

    const result = await getPreferences();

    expect(receivedCommand).toBe("get_preferences");
    expect(result).toEqual(preferences);
  });

  it("setTheme sends the chosen theme", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return { theme: "light", checkForUpdates: true };
    });

    await setTheme("light");

    expect(receivedArgs).toEqual({ theme: "light" });
  });

  it("checkForUpdate invokes check_for_update with the given trigger and returns the outcome", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const outcome: UpdateCheckOutcomeDto = { state: "upToDate", currentTag: "v0.4.0" };
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return outcome;
    });

    const result = await checkForUpdate("manual");

    expect(receivedCommand).toBe("check_for_update");
    expect(receivedArgs).toEqual({ trigger: "manual" });
    expect(result).toEqual(outcome);
  });

  it("setCheckForUpdates sends the enabled flag", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return { theme: "dark", checkForUpdates: false };
    });

    const result = await setCheckForUpdates(false);

    expect(receivedArgs).toEqual({ enabled: false });
    expect(result.checkForUpdates).toBe(false);
  });

  it("openUpdateLink sends the url and returns whether it was opened", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return true;
    });

    const opened = await openUpdateLink("https://github.com/rpaggi/gitsail/releases/tag/v0.5.0");

    expect(receivedCommand).toBe("open_update_link");
    expect(receivedArgs).toEqual({
      url: "https://github.com/rpaggi/gitsail/releases/tag/v0.5.0",
    });
    expect(opened).toBe(true);
  });
});
