# GitSail for VS Code

EPIC-14 — VS Code Foundation (US-068–US-071). This extension gives VS Code
repository context (which repository the active file belongs to, its
current branch/HEAD state) by asking the `gitsail-cli` process, and nothing
else — it never re-implements Git detection, log parsing, or blame parsing
on the extension side (SAD §16, ADR-007). Blame/history/diff UI is EPIC-15's
job; this foundation only wires activation, the CLI client, binary
discovery/verification, and workspace lifecycle/trust.

## Architecture

```
VS Code Extension (TypeScript)
          |
   GitSail Client        (src/cliClient.ts — one process per query,
          |                envelope/schemaVersion validated, US-069)
 CLI JSON initially
          |
 gitsail-protocol         (src/protocol.ts, src/dto.ts — hand mirrors)
          |
 Application/Core
```

Module map (see each file's own doc comment for the acceptance criterion it
implements):

| File | Story | Responsibility |
|---|---|---|
| `src/protocol.ts` | US-069 | Envelope/schemaVersion validation — mirrors `gitsail-protocol`. |
| `src/dto.ts` | US-069 | Hand-mirrored DTO shapes (currently just `RepositoryDto`). |
| `src/cliClient.ts` | US-069 | Spawns `gitsail-cli --json`, one process per query; timeout/cancel/cleanup. |
| `src/cliErrors.ts` | US-069 | Typed client-level failure hierarchy (never a domain error). |
| `src/cliLocator.ts` | US-070 | Resolves and verifies the binary (PATH vs. configured path; version check). |
| `src/repositoryContext.ts` | US-068 | Associates the active file with its repository via `gitsail open`. |
| `src/workspaceTrust.ts` | US-071 | Wraps `vscode.workspace.isTrusted`/`onDidGrantWorkspaceTrust`. |
| `src/documentState.ts` | US-071 | Classifies unsaved-buffer vs. on-disk state. |
| `src/hostTypes.ts` / `src/controller.ts` | all four | The testable orchestration layer, decoupled from the real `vscode` module. |
| `src/extension.ts` | — | The one file that imports the real `vscode` module; adapts it to `hostTypes.ts` and hands it to `controller.ts`. |

### Why `src/controller.ts` never imports `vscode`

The real VS Code extension API only exists inside a running extension host —
there is no npm-installable `vscode` runtime package to `import` from a
plain Node test process. Rather than mock the `vscode` module itself, all
business logic (`controller.ts` and everything it drives) is written
against `ExtensionHost` (`src/hostTypes.ts`), a small interface capturing
exactly the slice of the real API this extension touches. `extension.ts` is
the only file that adapts the real `vscode` namespace to that interface;
every unit test in `test/` drives the same interface with a plain object,
never a `vscode` mock.

## Minimum VS Code version

`engines.vscode` is pinned to `^1.85.0` (November 2023). Chosen because
Workspace Trust (`vscode.workspace.isTrusted` / `onDidGrantWorkspaceTrust`,
used by US-071) has been generally available and stable since well before
that release, and 1.85 is old enough to cover a realistic range of current
installs without carrying years of unrelated API baggage.

## Configuration

- `gitsail.binaryPath` (string, default `""`): absolute path to the
  `gitsail` executable. Only honored in a **trusted** workspace (see
  "Workspace trust" below). Leave empty to discover `gitsail`/`gitsail.exe`
  on `PATH`.

## Binary distribution (US-070 criterion 2)

**Decision, registered before v0.4 packaging:** this extension does **not**
bundle a per-platform `gitsail-cli` binary in v0.4. It requires the user to
install `gitsail` themselves (the same binary EPIC-08 ships for the CLI/TUI)
and either put it on `PATH` or point `gitsail.binaryPath` at it.

**Why:** EPIC-25 (Distribution & Updates) — the epic that would define a
real, signed, per-OS/arch release pipeline — has not shipped yet at this
point in the roadmap. Bundling a hand-built, unsigned binary now would
undermine US-070 criterion 3 ("origin/version are verifiable; no silent
download/execution of an untrusted file") in spirit: the extension would be
trusting a binary of unclear provenance just because it shipped inside the
`.vsix`, rather than because its version was actually checked. Requiring an
explicit, user-controlled install keeps that verification meaningful.

The full writeup lives in `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`,
**ADR-015 — VS Code binary distribution for v0.4**. Revisit this decision
once EPIC-25 defines a pipeline that can produce a signed, verifiable
per-platform artifact this extension could embed.

**Verification, not blind trust (criterion 3):** before ever calling
`gitsail open`/etc., the extension spawns `<binary> --version`, parses the
result, and compares it against a minimum supported version
(`MINIMUM_SUPPORTED_CLI_VERSION` in `src/cliLocator.ts`). A missing binary,
an unrecognized `--version` output (most likely a different program at that
path), or a version below the minimum are all distinct, clearly reported
states — none of them fall back to any alternate Git parsing.

## Workspace trust (US-071 criterion 2)

US-071 also depends on US-110 (EPIC-22 — Security & Safety), which has no
corresponding epic/task in the board yet. Rather than block on that, this
extension implements criterion 2 directly against VS Code's own native
workspace trust API: an untrusted workspace's `gitsail.binaryPath` setting
is never read (`src/cliLocator.ts::resolveBinaryCommand`), and the extension
surfaces a clear, one-time notice instead of silently ignoring the setting.
If/when US-110 defines a broader security policy, `src/workspaceTrust.ts`
is the one place that would grow to also honor it.

## Unsaved buffers (US-071 criterion 3)

`gitsail-cli` only ever reads what is actually saved on disk. `src/documentState.ts` classifies the active document's dirty/saved state, and the
controller logs an explicit note whenever the active file has unsaved
changes, instead of silently presenting CLI results as if they described
the in-editor buffer. Attaching that note to a specific piece of UI (e.g. a
decoration) is EPIC-15's job once there is blame/history content to attach
it to.

## Testing

```bash
npm install
npm run lint    # tsc --noEmit
npm run build   # tsc -p ./
npm test        # vitest run — unit + contract tests, no real vscode module
```

All business logic (`protocol.ts`, `dto.ts`, `cliClient.ts`, `cliLocator.ts`,
`repositoryContext.ts`, `workspaceTrust.ts`, `documentState.ts`,
`controller.ts`) is covered by Vitest, either as pure unit tests or as
contract tests against a real spawned process (`test/fixtures/fake-cli.js`,
a small Node script standing in for `gitsail-cli`, mirroring
`crates/gitsail-cli/tests/fixtures/fake-git`'s existing pattern in the Rust
CLI's own integration tests).

### Real extension-host test — sandbox limitation

`test-e2e/` scaffolds a real `@vscode/test-electron` smoke test that
downloads an actual VS Code build and activates this extension inside it —
the one thing the Vitest suite above cannot exercise, since it never
touches the real `vscode` module by design.

This was attempted in this project's development sandbox
(`npm run test:e2e`, i.e. `node test-e2e/runTest.js`): downloading VS Code
1.138.0 succeeded (328 MB, `.vscode-test/`, gitignored), but launching it
failed —

```
[...] ERROR:ui/ozone/platform/x11/ozone_platform_x11.cc:257] Missing X server or $DISPLAY
[...] ERROR:ui/aura/env.cc:246] The platform failed to initialize.  Exiting.
```

There is no `Xvfb`/`xvfb-run` installed in this sandbox, and provisioning a
virtual display server is an environment change outside this story's
scope. This mirrors `apps/desktop`'s own documented gap for its Tauri smoke
test (see Takumi task T-184: no display/WebKitGTK in that same sandbox).
`npm run test:e2e` should work as-is on a developer machine or a
GUI-capable CI runner (SAD §35 lists "VS Code extension test/build" as a
pipeline stage); running it there — and wiring it into CI — is left as a
manual/CI follow-up rather than something this story could verify here.
