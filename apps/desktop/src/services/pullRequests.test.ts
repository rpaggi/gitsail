import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { listPullRequests, openPullRequestLink } from "./pullRequests";
import type { ListPullRequestsOutcomeDto } from "./dto";

describe("pull requests service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("listPullRequests invokes list_pull_requests with the given page and returns the outcome as-is", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    const outcome: ListPullRequestsOutcomeDto = { state: "noForgeDetected" };
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return outcome;
    });

    const result = await listPullRequests(2);

    expect(receivedCommand).toBe("list_pull_requests");
    expect(receivedArgs).toEqual({ page: 2 });
    expect(result).toEqual(outcome);
  });

  it("openPullRequestLink invokes open_pull_request_link with the given url", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return true;
    });

    const opened = await openPullRequestLink("https://github.com/org/repo/pull/1");

    expect(receivedCommand).toBe("open_pull_request_link");
    expect(receivedArgs).toEqual({ url: "https://github.com/org/repo/pull/1" });
    expect(opened).toBe(true);
  });

  it("openPullRequestLink returns false without a rejected promise when the backend refuses the host", async () => {
    mockIPC(() => false);

    const opened = await openPullRequestLink("https://evil.example/org/repo/pull/1");

    expect(opened).toBe(false);
  });
});
