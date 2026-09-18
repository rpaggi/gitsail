import { describe, expect, it } from "vitest";

import {
  dropAction,
  parseDragPayload,
  serializeDragPayload,
  type StagingDragPayload,
} from "./dragDropStaging";

describe("dropAction", () => {
  it("dragging from unstaged onto staged stages the file", () => {
    expect(dropAction({ path: "a.txt", sourceZone: "unstaged" }, "staged")).toBe("stage");
  });

  it("dragging from staged onto unstaged unstages the file", () => {
    expect(dropAction({ path: "a.txt", sourceZone: "staged" }, "unstaged")).toBe("unstage");
  });

  it("dropping back onto the same zone is not a recognized action", () => {
    expect(dropAction({ path: "a.txt", sourceZone: "unstaged" }, "unstaged")).toBeNull();
    expect(dropAction({ path: "a.txt", sourceZone: "staged" }, "staged")).toBeNull();
  });
});

describe("drag payload serialization", () => {
  it("round-trips a valid payload", () => {
    const payload: StagingDragPayload = { path: "src/a.txt", sourceZone: "unstaged" };

    const parsed = parseDragPayload(serializeDragPayload(payload));

    expect(parsed).toEqual(payload);
  });

  it("rejects malformed JSON instead of guessing a path from it", () => {
    expect(parseDragPayload("not json")).toBeNull();
  });

  it("rejects well-formed JSON that is not a staging payload (e.g. a foreign OS drag)", () => {
    expect(parseDragPayload(JSON.stringify({ some: "other drag data" }))).toBeNull();
    expect(parseDragPayload(JSON.stringify({ path: "a.txt", sourceZone: "somewhere-else" }))).toBeNull();
    expect(parseDragPayload(JSON.stringify({ path: 42, sourceZone: "staged" }))).toBeNull();
  });
});
