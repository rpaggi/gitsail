import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { usePullRequestsStore } from "./pullRequests";
import type { ListPullRequestsOutcomeDto, PullRequestSummaryDto } from "../services/dto";

function samplePullRequest(overrides: Partial<PullRequestSummaryDto> = {}): PullRequestSummaryDto {
  return {
    title: "Fix the thing",
    state: "open",
    author: "octocat",
    sourceBranch: "feature/fix",
    targetBranch: "main",
    url: "https://github.com/org/repo/pull/1",
    ...overrides,
  };
}

describe("pull requests store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("starts idle before anything loads", () => {
    const store = usePullRequestsStore();
    expect(store.status).toBe("idle");
    expect(store.page).toBeNull();
  });

  it("load('page') with items marks the store loaded and stores the page", async () => {
    const outcome: ListPullRequestsOutcomeDto = {
      state: "page",
      page: { items: [samplePullRequest()], hasNextPage: true },
    };
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") return outcome;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("loaded");
    expect(store.page?.items).toHaveLength(1);
    expect(store.page?.hasNextPage).toBe(true);
  });

  it("a truly empty page is still 'loaded', distinct from every error state", async () => {
    const outcome: ListPullRequestsOutcomeDto = { state: "page", page: { items: [], hasNextPage: false } };
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") return outcome;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("loaded");
    expect(store.page?.items).toHaveLength(0);
  });

  it("noForgeDetected maps to the noForge status", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") return { state: "noForgeDetected" } satisfies ListPullRequestsOutcomeDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("noForge");
    expect(store.page).toBeNull();
  });

  it("authenticationRequired and permissionDenied both map to the noToken status", async () => {
    const store = usePullRequestsStore();

    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return { state: "authenticationRequired" } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    await store.load(1);
    expect(store.status).toBe("noToken");

    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return { state: "permissionDenied" } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    await store.load(1);
    expect(store.status).toBe("noToken");
  });

  it("rateLimited records the reported wait time", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return { state: "rateLimited", retryAfterSeconds: 42 } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("rateLimited");
    expect(store.retryAfterSeconds).toBe(42);
  });

  it("rateLimited without a reported wait time still reports the state distinctly", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") return { state: "rateLimited" } satisfies ListPullRequestsOutcomeDto;
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("rateLimited");
    expect(store.retryAfterSeconds).toBeNull();
  });

  it("offline records the redacted diagnostic message, never as an empty page", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return { state: "offline", message: "connection refused" } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("offline");
    expect(store.offlineMessage).toBe("connection refused");
    expect(store.page).toBeNull();
  });

  it("error records the message and is distinct from every other state", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return {
          state: "error",
          error: { code: "internal", message: "something unexpected happened" },
        } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.status).toBe("error");
    expect(store.errorMessage).toBe("something unexpected happened");
  });

  it("nextPage/previousPage delegate to load with the right page number", async () => {
    const received: number[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_pull_requests") {
        received.push((args as { page: number }).page);
        return {
          state: "page",
          page: { items: [], hasNextPage: true },
        } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.loadFirstPage();
    await store.nextPage();
    await store.nextPage();
    await store.previousPage();

    expect(received).toEqual([1, 2, 3, 2]);
  });

  it("previousPage is a no-op on the first page", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "list_pull_requests") {
        return { state: "page", page: { items: [], hasNextPage: false } } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();
    await store.loadFirstPage();
    received.length = 0;

    await store.previousPage();

    expect(received).toEqual([]);
  });

  it("nextPage is a no-op when the current page reports no next page", async () => {
    const received: string[] = [];
    mockIPC((cmd) => {
      received.push(cmd);
      if (cmd === "list_pull_requests") {
        return { state: "page", page: { items: [], hasNextPage: false } } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();
    await store.loadFirstPage();
    received.length = 0;

    await store.nextPage();

    expect(received).toEqual([]);
  });

  it("openInBrowser delegates to open_pull_request_link with the item's own url", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "open_pull_request_link") {
        receivedArgs = args;
        return true;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.openInBrowser("https://github.com/org/repo/pull/1");

    expect(receivedArgs).toEqual({ url: "https://github.com/org/repo/pull/1" });
  });

  // US-103 criterion 3: the store itself never sanitizes/transforms
  // untrusted content — that is exclusively the template's job (Vue's
  // default text interpolation; see `pullRequestsPresentation.test.ts`'s
  // own guard that `PullRequestsPanel.vue` never uses `v-html`). This test
  // documents that the store is a faithful pass-through, not a place a
  // future change might "helpfully" start interpreting Markdown/HTML.
  it("never alters malicious title/author content — sanitization is the template's job, not the store's", async () => {
    const malicious = samplePullRequest({
      title: "<script>alert(1)</script>",
      author: "[click](javascript:alert(1))",
    });
    mockIPC((cmd) => {
      if (cmd === "list_pull_requests") {
        return { state: "page", page: { items: [malicious], hasNextPage: false } } satisfies ListPullRequestsOutcomeDto;
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    const store = usePullRequestsStore();

    await store.load(1);

    expect(store.page?.items[0].title).toBe("<script>alert(1)</script>");
    expect(store.page?.items[0].author).toBe("[click](javascript:alert(1))");
  });
});
