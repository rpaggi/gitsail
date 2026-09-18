import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import { listStashEntries, listTags } from "./references";
import type { StashDto, TagDto } from "./dto";

function sampleTags(): TagDto[] {
  return [
    { name: "v1.0", target: "a".repeat(40), kind: { kind: "lightweight" } },
    {
      name: "v2.0",
      target: "b".repeat(40),
      kind: {
        kind: "annotated",
        message: "Release 2.0",
        tagger: { name: "Ada", email: "ada@example.com" },
        date: { secondsSinceEpoch: 1_700_000_000, utcOffsetMinutes: 0 },
      },
    },
  ];
}

function sampleStashes(): StashDto[] {
  return [
    {
      index: 0,
      commit: "a".repeat(40),
      message: "WIP on main",
      date: { secondsSinceEpoch: 1_700_000_500, utcOffsetMinutes: 0 },
    },
  ];
}

describe("references service", () => {
  afterEach(() => {
    clearMocks();
  });

  it("listTags invokes list_tags and returns every tag", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return sampleTags();
    });

    const tags = await listTags();

    expect(receivedCommand).toBe("list_tags");
    expect(tags).toHaveLength(2);
    expect(tags[1].kind.kind).toBe("annotated");
  });

  it("listStashEntries invokes list_stash_entries and returns every entry", async () => {
    let receivedCommand = "";
    mockIPC((cmd) => {
      receivedCommand = cmd;
      return sampleStashes();
    });

    const stashes = await listStashEntries();

    expect(receivedCommand).toBe("list_stash_entries");
    expect(stashes).toHaveLength(1);
    expect(stashes[0].message).toBe("WIP on main");
  });
});
