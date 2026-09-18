import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { createBranch, deleteBranch, listBranches, switchBranch } from "./branches";
import type { BranchDto } from "./dto";

function sampleBranches(): BranchDto[] {
  return [
    {
      name: "main",
      kind: { kind: "local" },
      target: "a".repeat(40),
      upstream: null,
      ahead: 0,
      behind: 0,
      isCurrent: true,
    },
    {
      name: "feature/x",
      kind: { kind: "local" },
      target: "b".repeat(40),
      upstream: null,
      ahead: 1,
      behind: 2,
      isCurrent: false,
    },
  ];
}

describe("branches service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("listBranches invokes list_branches and returns every branch", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return sampleBranches();
    });

    const branches = await listBranches();

    expect(receivedCommand).toBe("list_branches");
    expect(branches).toHaveLength(2);
    expect(branches[1].name).toBe("feature/x");
  });

  it("createBranch sends the name and an omitted start point as null", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await createBranch("feature/y");

    expect(receivedCommand).toBe("create_branch");
    expect(receivedArgs).toEqual({ name: "feature/y", startPoint: null });
  });

  it("createBranch forwards an explicit start point", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return null;
    });

    await createBranch("feature/y", "main");

    expect(receivedArgs).toEqual({ name: "feature/y", startPoint: "main" });
  });

  it("switchBranch sends the target branch name", async () => {
    let receivedCommand = "";
    let receivedArgs: unknown;
    mockIPC((cmd, args) => {
      receivedCommand = cmd;
      receivedArgs = args;
      return null;
    });

    await switchBranch("develop");

    expect(receivedCommand).toBe("switch_branch");
    expect(receivedArgs).toEqual({ target: "develop" });
  });

  it("deleteBranch forwards the force flag exactly as given", async () => {
    let receivedArgs: unknown;
    mockIPC((_cmd, args) => {
      receivedArgs = args;
      return null;
    });

    await deleteBranch("feature/x", true);

    expect(receivedArgs).toEqual({ name: "feature/x", force: true });
  });
});
