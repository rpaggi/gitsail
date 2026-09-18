import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { useAmendStore } from "./amend";
import { useOperationStore } from "./operation";
import type { AmendPreviewDto } from "../services/dto";

function samplePreview(subject: string, body = ""): AmendPreviewDto {
  return {
    head: {
      hash: "a".repeat(40),
      shortHash: "aaaaaaaa",
      parents: [],
      author: { name: "Ada", email: "ada@example.com" },
      committer: { name: "Ada", email: "ada@example.com" },
      authorDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      commitDate: { secondsSinceEpoch: 0, utcOffsetMinutes: 0 },
      subject,
      body,
      decorations: [],
      isMerge: false,
      isRoot: false,
    },
    stagedDiff: { files: [] },
  };
}

describe("amend store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loadPreview pre-fills the message from HEAD's current subject", async () => {
    mockIPC((cmd) => {
      if (cmd === "preview_amend") return samplePreview("fix the bug");
      return null;
    });
    const store = useAmendStore();

    await store.loadPreview();

    expect(store.message).toBe("fix the bug");
    expect(store.preview?.head.hash).toBe("a".repeat(40));
  });

  it("loadPreview combines subject and body when a body is present", async () => {
    mockIPC(() => samplePreview("fix the bug", "more detail here"));
    const store = useAmendStore();

    await store.loadPreview();

    expect(store.message).toBe("fix the bug\n\nmore detail here");
  });

  it("requestAmend is Destructive risk with an explicit rewrite-history warning", async () => {
    mockIPC(() => samplePreview("fix the bug"));
    const store = useAmendStore();
    await store.loadPreview();

    await store.requestAmend();

    const operation = useOperationStore();
    expect(operation.status).toBe("confirming");
    expect(operation.current?.risk).toBe("destructive");
    expect(operation.current?.impact).toContain("rewriting");
    expect(operation.current?.impact).not.toBe("");
  });

  it("requestAmend without a loaded preview is a no-op", async () => {
    const store = useAmendStore();

    await store.requestAmend();

    const operation = useOperationStore();
    expect(operation.status).toBe("idle");
  });

  it("confirming sends back exactly the previewed HEAD hash and clears the preview on success", async () => {
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      if (cmd === "preview_amend") return samplePreview("fix the bug");
      if (cmd === "amend_commit") {
        receivedArgs = args;
        return { hash: "b".repeat(40) };
      }
      if (cmd === "get_repository_status") {
        return { branch: "main", headState: { state: "attached", branch: "main" }, files: [], isClean: true };
      }
      return null;
    });
    const store = useAmendStore();
    await store.loadPreview();
    store.message = "amended message";

    await store.requestAmend();
    const operation = useOperationStore();
    await operation.confirm();

    expect(receivedArgs).toEqual({ message: "amended message", expectedHead: "a".repeat(40) });
    expect(store.lastCommitHash).toBe("b".repeat(40));
    expect(store.preview).toBeNull();
    expect(operation.status).toBe("succeeded");
  });

  it("a stale HEAD reports a conflict rather than a generic error", async () => {
    mockIPC((cmd) => {
      if (cmd === "preview_amend") return samplePreview("fix the bug");
      if (cmd === "amend_commit") {
        throw { code: "operation_conflict", message: "HEAD changed since the amend was previewed" };
      }
      return null;
    });
    const store = useAmendStore();
    await store.loadPreview();

    await store.requestAmend();
    const operation = useOperationStore();
    await operation.confirm();

    expect(operation.status).toBe("failed");
    expect(operation.error?.message).toBe("HEAD changed since the amend was previewed");
  });
});
