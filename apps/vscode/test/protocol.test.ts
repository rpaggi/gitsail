import { describe, expect, it } from "vitest";

import { EnvelopeParseError, parseEnvelope, UnsupportedSchemaVersionError } from "../src/protocol";

describe("parseEnvelope", () => {
  it("parses a valid ok envelope", () => {
    const envelope = parseEnvelope<{ branch: string }>(
      JSON.stringify({
        status: "ok",
        schemaVersion: 1,
        requestId: "req-1",
        data: { branch: "main" },
      }),
    );

    expect(envelope.status).toBe("ok");
    if (envelope.status === "ok") {
      expect(envelope.data.branch).toBe("main");
      expect(envelope.requestId).toBe("req-1");
    }
  });

  it("parses a valid error envelope", () => {
    const envelope = parseEnvelope(
      JSON.stringify({
        status: "error",
        schemaVersion: 1,
        requestId: "req-2",
        error: { code: "repository_not_found", message: "not a Git repository" },
      }),
    );

    expect(envelope.status).toBe("error");
    if (envelope.status === "error") {
      expect(envelope.error.code).toBe("repository_not_found");
    }
  });

  it("rejects output that is not JSON at all", () => {
    expect(() => parseEnvelope("not json")).toThrow(EnvelopeParseError);
  });

  it("rejects a JSON value that is not an object", () => {
    expect(() => parseEnvelope("42")).toThrow(EnvelopeParseError);
  });

  it("rejects an envelope missing schemaVersion", () => {
    expect(() =>
      parseEnvelope(JSON.stringify({ status: "ok", requestId: "req-1", data: {} })),
    ).toThrow(EnvelopeParseError);
  });

  it("rejects an envelope missing requestId", () => {
    expect(() =>
      parseEnvelope(JSON.stringify({ status: "ok", schemaVersion: 1, data: {} })),
    ).toThrow(EnvelopeParseError);
  });

  it("rejects an unsupported schemaVersion distinctly from a malformed envelope", () => {
    expect(() =>
      parseEnvelope(
        JSON.stringify({ status: "ok", schemaVersion: 999, requestId: "req-1", data: {} }),
      ),
    ).toThrow(UnsupportedSchemaVersionError);
  });

  it("rejects an ok envelope missing data", () => {
    expect(() =>
      parseEnvelope(JSON.stringify({ status: "ok", schemaVersion: 1, requestId: "req-1" })),
    ).toThrow(EnvelopeParseError);
  });

  it("rejects an error envelope with an invalid error payload", () => {
    expect(() =>
      parseEnvelope(
        JSON.stringify({
          status: "error",
          schemaVersion: 1,
          requestId: "req-1",
          error: { code: "x" },
        }),
      ),
    ).toThrow(EnvelopeParseError);
  });

  it("rejects an unrecognized status value", () => {
    expect(() =>
      parseEnvelope(
        JSON.stringify({ status: "maybe", schemaVersion: 1, requestId: "req-1" }),
      ),
    ).toThrow(EnvelopeParseError);
  });
});
