Compatibility fixtures shared by `gitsail-protocol`'s and the VS Code
extension's automated tests (US-039 criterion 3), so both languages assert
against byte-identical payloads instead of two hand-maintained copies that
could quietly drift apart.

- `ok-v1.json` / `error-v1.json` — a real `gitsail status --json` shape
  under the schema version this repository currently produces
  (`gitsail_protocol::SCHEMA_VERSION`, currently `1`). Both consumers must
  accept these.
- `unsupported-v2.json` — a **hypothetical** next schema version, invented
  only for this test suite (nothing in this repository actually produces
  it). It models a plausible real redesign — the envelope's tag/field names
  change (`status`/`data`/`error` become `ok`/`result`) rather than merely
  adding a field — so a consumer that skipped the `schemaVersion` check
  would find no `data` to misread, not a coincidentally-compatible shape.
  Both consumers must reject it, and must do so by checking
  `schemaVersion` alone, before ever attempting to interpret `result` as
  if it were `data`.

Consumers:
- Rust: `crates/gitsail-protocol/tests/compatibility.rs` (via
  `gitsail_protocol::parse_envelope`).
- TypeScript: `apps/vscode/test/protocolCompatibility.test.ts` (via
  `parseEnvelope` from `apps/vscode/src/protocol.ts`).

See `docs/architecture/protocol-compatibility.md` for the supported-version
matrix these fixtures back, and `crates/gitsail-protocol/src/envelope.rs`'s
`SCHEMA_VERSION` doc comment for the policy on when to add a new
`unsupported-vN.json` here alongside a real schema bump.
