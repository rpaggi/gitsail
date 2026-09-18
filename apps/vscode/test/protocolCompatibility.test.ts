// Cross-language compatibility contract (US-039 criteria 2-3): this suite
// reads the exact same JSON payloads `crates/gitsail-protocol/tests/compatibility.rs`
// reads, from `docs/architecture/fixtures/protocol-compatibility/` (see
// that directory's README for why they are shared verbatim instead of two
// hand-maintained copies), and asserts this extension's own consumer-side
// guard (`parseEnvelope`) accepts the supported schema version and rejects
// the hypothetical unsupported one — the TypeScript half of the same
// contract `gitsail_protocol::parse_envelope` enforces in Rust.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { parseEnvelope, UnsupportedSchemaVersionError } from "../src/protocol";

const FIXTURES_DIR = join(__dirname, "..", "..", "..", "docs", "architecture", "fixtures", "protocol-compatibility");

function readFixture(name: string): string {
  return readFileSync(join(FIXTURES_DIR, name), "utf8");
}

interface StatusLikeData {
  branch: string | null;
  isClean: boolean;
  files: unknown[];
}

describe("protocol compatibility fixtures", () => {
  it("accepts the supported schema version ok fixture and decodes its data", () => {
    const envelope = parseEnvelope<StatusLikeData>(readFixture("ok-v1.json"));

    expect(envelope.status).toBe("ok");
    if (envelope.status === "ok") {
      expect(envelope.data.branch).toBe("main");
      expect(envelope.data.isClean).toBe(false);
      expect(envelope.data.files).toHaveLength(2);
    }
  });

  it("accepts the supported schema version error fixture", () => {
    const envelope = parseEnvelope(readFixture("error-v1.json"));

    expect(envelope.status).toBe("error");
    if (envelope.status === "error") {
      expect(envelope.error.code).toBe("repository_not_found");
    }
  });

  it("rejects the hypothetical unsupported schema version before touching its payload", () => {
    const raw = readFixture("unsupported-v2.json");

    // The hypothetical v2 shape has no `data` key at all (it uses `result`
    // instead) — so if this ever threw a generic parse error instead of
    // `UnsupportedSchemaVersionError`, it would mean the schemaVersion gate
    // was skipped and a decode was attempted (and failed) on `data`, not
    // rejected up front purely on the version number.
    expect(() => parseEnvelope(raw)).toThrow(UnsupportedSchemaVersionError);

    try {
      parseEnvelope(raw);
      expect.unreachable("parseEnvelope must throw for an unsupported schemaVersion");
    } catch (err) {
      expect(err).toBeInstanceOf(UnsupportedSchemaVersionError);
      expect((err as UnsupportedSchemaVersionError).schemaVersion).toBe(2);
    }
  });
});
