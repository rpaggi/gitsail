# Protocol compatibility matrix (v0.4)

Companion to ADR-008, ADR-014 and ADR-016 in
[`GitSail_SAD_and_ADRs_v0.1.md`](./GitSail_SAD_and_ADRs_v0.1.md), and to
`crates/gitsail-protocol/src/envelope.rs`'s `SCHEMA_VERSION` doc comment,
which is the canonical statement of the versioning policy. This document is
the release-facing artifact that policy asks to be kept current: which
component supports which `schemaVersion`(s) right now, and where the
compatibility tests that enforce it live. Update it in the same change that
touches `SCHEMA_VERSION` or a component's supported-version list.

## Two independent version numbers

GitSail has two version concepts that are easy to conflate but never move
together:

| | What it identifies | Where it lives | Checked by |
|---|---|---|---|
| `schemaVersion` | The shape of one `Envelope<T>` on the wire | `gitsail_protocol::SCHEMA_VERSION` | `gitsail_protocol::parse_envelope` (Rust), `parseEnvelope`/`SUPPORTED_SCHEMA_VERSIONS` (`apps/vscode/src/protocol.ts`) |
| `gitsail-cli` binary version | The CLI executable's own release version | `Cargo.toml` (`gitsail-cli`, currently `0.0.0` workspace-wide — ADR-013) | `apps/vscode/src/cliLocator.ts`'s `MINIMUM_SUPPORTED_CLI_VERSION` probe (ADR-015) |

A binary can be new enough per `cliLocator.ts` and still emit a
`schemaVersion` a given extension build does not speak (or vice versa,
once the CLI has a real release cadence ahead of `SCHEMA_VERSION` changes).
The two checks are independent and both required.

## Supported `schemaVersion` per component, v0.4

| Component | Role | Crosses an independently-versioned process boundary? | Supported `schemaVersion`(s) |
|---|---|---|---|
| `gitsail-cli` (`crates/gitsail-cli`) | Producer — the only thing that writes an `Envelope` to the outside world today | Yes — its stdout is read by other processes | Produces `1` (`gitsail_protocol::SCHEMA_VERSION`) |
| VS Code extension (`apps/vscode`) | Consumer, over CLI JSON (ADR-012, ADR-015) | Yes — spawns a separately-installed `gitsail` binary | `[1]` (`SUPPORTED_SCHEMA_VERSIONS` in `apps/vscode/src/protocol.ts`) |
| TUI (`crates/gitsail-tui`) | N/A — calls `gitsail-application`/`gitsail-domain` directly | No — single binary, one `Cargo.lock`, never links `gitsail-protocol` | Not applicable (SAD §18) |
| Desktop (`apps/desktop`) | N/A on the wire — Tauri commands return `gitsail-protocol` DTOs straight to the Vue webview over Tauri's own IPC, not a `gitsail-cli`-produced `Envelope` | No — frontend and backend ship as one signed app build | Not applicable today (SAD §17); would need `schemaVersion` if/when Desktop's IPC is versioned independently of its Rust backend |
| Future daemon/IPC transport (ADR-012, "open architecture decisions" §39) | Producer and/or consumer | Yes, by construction | Not built yet — `gitsail_protocol::parse_envelope`/`SUPPORTED_SCHEMA_VERSIONS` exist specifically so this transport has a ready-made runtime check instead of inventing one later |

**Why TUI and Desktop have no runtime `schemaVersion` check (US-039
criterion 2's evaluation):** both are compiled from the same workspace
`Cargo.lock` as the `gitsail-protocol` crate they use, in the same build,
shipped as one artifact. There is no scenario today where a TUI or Desktop
binary observes a `gitsail-protocol` DTO shape from a *different* build of
that crate — Rust's own type system is the compatibility check, enforced
at compile time. This only changes if a local daemon/IPC transport
(ADR-012) lets two independently-released Rust binaries talk to each
other; `gitsail_protocol::compat` (`parse_envelope`,
`SUPPORTED_SCHEMA_VERSIONS`) exists now so that transport does not have to
invent this check from scratch — see its module doc comment.

## Compatibility fixtures

`docs/architecture/fixtures/protocol-compatibility/` holds the payloads
both the Rust and TypeScript compatibility suites assert against, so the
two languages check the same bytes instead of two hand-maintained copies
drifting apart:

- `ok-v1.json`, `error-v1.json` — a real `gitsail status --json` shape
  under `schemaVersion: 1`, the version this repository currently
  produces. Every consumer above must accept these.
- `unsupported-v2.json` — a **hypothetical** next schema version invented
  only for these tests (nothing in this repository actually produces it).
  It reshapes the envelope's own tag/field names (`status`/`data`/`error`
  become `ok`/`result`), not merely adds a field, so it genuinely exercises
  "reject before misreading," not "reject a shape that happens to also
  parse fine." Every consumer above must reject it, and must do so from
  `schemaVersion` alone.

See that directory's `README.md` for the exact rationale, and:

- `crates/gitsail-protocol/tests/compatibility.rs` — Rust consumer-side
  acceptance/rejection against these fixtures.
- `crates/gitsail-protocol/tests/dto_wire_shape.rs` — pins representative
  DTOs' exact wire shape, independent of the fixtures above, so a silent
  `#[serde(...)]` attribute change (rename, re-tag, drop
  `skip_serializing_if`) fails a test instead of shipping unnoticed.
- `crates/gitsail-cli/tests/protocol_compatibility.rs` — end-to-end: spawns
  the real `gitsail` binary as producer and feeds its actual stdout to
  `gitsail_protocol::parse_envelope` as consumer, plus one hand-crafted
  unsupported-version rejection (the real producer cannot yet emit an
  incompatible version, by this story's own design).
- `apps/vscode/test/protocolCompatibility.test.ts` — the TypeScript half of
  the same fixture-based contract, via `parseEnvelope`.

## When this table must change

Whenever `SCHEMA_VERSION` moves (per the policy in `envelope.rs`), or a
component's supported-version list changes (e.g. an extension release
drops support for an old `schemaVersion`, or the CLI starts producing a
new one before all consumers have caught up):

1. Update the relevant "supported `schemaVersion`(s)" cell(s) above.
2. Add a new `docs/architecture/fixtures/protocol-compatibility/*.json`
   fixture for the new version if its shape is now real (not hypothetical)
   and used by an accept-side test somewhere in the matrix.
3. Keep at least one *reject*-side fixture for a version nothing here
   supports, so the "rejects unknown schema" half of the contract never
   goes untested once the current `unsupported-v2.json` becomes a real,
   supported version instead of a hypothetical one.
