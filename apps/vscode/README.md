# GitSail for VS Code

EPIC-14 — VS Code Foundation (US-068–US-071) plus EPIC-15 — Blame & History
(US-072–US-077). This extension gives VS Code repository context, inline
blame, and commit/file/line history by asking the `gitsail-cli` process, and
nothing else — it never re-implements Git detection, log parsing, diffing,
or blame parsing on the extension side (SAD §16, ADR-007).

## Installation (T-259/US-126)

**Official path today: "Install from VSIX...".** Download
`gitsail-vscode-<tag>.vsix` from the project's
[GitHub Releases](https://github.com/rpaggi/gitsail/releases) page (built
and checksummed by `.github/workflows/release.yml`, ADR-023), then in VS
Code: Extensions view → "..." menu → "Install from VSIX..." → select the
downloaded file. Equivalently, from a terminal: `code --install-extension
gitsail-vscode-<tag>.vsix`.

This extension is **not** published on the VS Code Marketplace or Open VSX
yet — a deliberate, registered decision (ADR-023), not an oversight; both
require a registered publisher account/token this project does not yet
have. `package.json`'s `publisher: "gitsail"` is an **unregistered
placeholder** — confirm or change it before ever running a real `vsce
publish`/`ovsx publish`. See
`docs/architecture/release-process.md`'s "Registered decision: VS Code
Marketplace / Open VSX stay unpublished" section for the exact checklist to
follow once those accounts exist.

This extension also does not bundle the `gitsail` CLI itself — see "Binary
distribution" below for why, and install `gitsail` separately from the same
GitHub Release (or build it from source).

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
| `src/extension.ts` | — | The one file that imports the real `vscode` module; adapts it to `hostTypes.ts`/`historyHostTypes.ts` and hands them to `controller.ts`/`historyController.ts`. |

### EPIC-15 (US-072–US-077)

| File | Story | Responsibility |
|---|---|---|
| `src/blameFormat.ts` | US-072 | Config parsing (`gitsail.blame.*`) and pure text formatting for one blame line — never invents an author for an uncommitted line or a dirty buffer. |
| `src/blamePlan.ts` | US-072 | Decides which lines get a decoration (current-line vs. all-visible-lines) from a full `BlameDto`. |
| `src/blameService.ts` | US-072 | `gitsail blame` wrapper plus client-side caches (blame per content-version, commit subjects by hash). |
| `src/hoverSanitizer.ts` | US-073 | Markdown escaping for repository text, plus `HoverContentBuilder.addCommandLink`'s scoped-trust command links — the security boundary preventing a malicious commit message from becoming an executable hover link. |
| `src/commitDetailsUri.ts` / `src/commitDetailsText.ts` | US-073 | `gitsail-commit:` read-only virtual document (plain text, not Markdown) showing one commit's full details. |
| `src/commitService.ts` | US-073/US-076 | `gitsail commit` / `gitsail commit-diff` / `gitsail show-file` wrappers. |
| `src/fileHistoryService.ts` / `src/lineHistoryService.ts` | US-074/US-075 | `gitsail log --path` / `gitsail line-history` wrappers, plus explicit-empty-state and disk-vs-buffer-caveat helpers. |
| `src/historyUri.ts` | US-076 | `gitsail-history:` read-only virtual document (a file's content at a revision) — never writes to a temp file, never touches the working tree. |
| `src/historyPresentation.ts` | US-074/US-075 | Pure `QuickPickItemLike[]` builders from history DTOs. |
| `src/desktopHandoff.ts` | US-077 | Builds/validates `--repo/--commit` arguments and launches GitSail Desktop, or reports a clear fallback when it is not configured/found. |
| `src/historyHostTypes.ts` / `src/historyController.ts` | all six | The testable orchestration layer for decorations/hover/commands/quickpicks/diff/content-providers, decoupled from the real `vscode` module — the EPIC-15 sibling of `hostTypes.ts`/`controller.ts`. |

### Configuration (EPIC-15/EPIC-21 — T-250/US-108)

- `gitsail.blame.enabled` (boolean, default `true`) — turns inline blame off entirely.
- `gitsail.blame.mode` (`"currentLine"` | `"allVisibleLines"`, default `"currentLine"`) — range: current line only, or every visible line.
- `gitsail.blame.delayMs` (number, default `400`; a negative value falls back to `400`) — inactivity delay before a decoration (re)appears.
- `gitsail.blame.format` (string template — `${author}`, `${authorEmail}`, `${date}`, `${hash}`, `${shortHash}`, `${message}`).
- `gitsail.blame.dateStyle` (`"absolute"` | `"relative"`, default `"absolute"`; an unrecognized value falls back to `"absolute"`) — controls `${date}` and the hover's date line: a fixed `YYYY-MM-DD`, or a relative duration like "3 days ago".
- `gitsail.desktop.path` (string, default `""`) — see "Desktop handoff" below.

Every one of the above is read once per decoration recompute via `readBlameDisplayConfig` (`blameFormat.ts`), which never throws on an invalid value — it always resolves to a documented default instead, so a bad setting can never turn into a broken decoration or a retry loop.

### Hover security (T-206 criterion 3)

A commit message is repository-authored, untrusted content. `hoverSanitizer.ts`'s
`escapeMarkdownText` neutralizes every Markdown control character in it before
it ever reaches a `vscode.MarkdownString`, so a message like
`[Click here](command:workbench.action.terminal.new)` can only ever render as
literal text. On top of that, `extension.ts` always constructs the hover's
`MarkdownString` with a **scoped** `isTrusted`:
`{ enabledCommands: [...] }`, listing only the specific command ids this
extension's own `HoverContentBuilder.addCommandLink` calls used (e.g.
`gitsail.openCommitDetails`) — never a blanket `true` (which would let any
`command:` link execute, including one hidden in imperfectly-escaped
repository text) and never a blanket `false` either (which would also
disable this extension's own legitimate "open full commit details" link).
`enabledCommands` is derived from what `HoverContentBuilder` actually built,
so it can never drift out of sync with the links the hover actually
contains.

### Historical content, never on disk (T-209 criterion 3)

Opening an old version of a file, or a commit's full details, never writes a
temp file: both are served through custom URI schemes
(`gitsail-history:` for file content at a revision, `gitsail-commit:` for
commit details) backed by a `vscode.TextDocumentContentProvider` this
extension registers. `vscode.workspace.openTextDocument`/`vscode.diff` open
these directly; VS Code has no save handler for either scheme, so the
resulting editor is naturally read-only and the working tree is never
touched.

### Desktop handoff — an acknowledged gap on the Desktop side (US-077)

`src/desktopHandoff.ts` implements and validates the full VS Code side of
the handoff (`gitsail-desktop --repo <path> --commit <hash>`), and
`gitsail.desktop.path` configures which executable to launch. **As of this
writing, `apps/desktop` does not parse any command-line arguments at all**
(`apps/desktop/src-tauri/src/{main,lib}.rs`'s `run()` takes none, and no
CLI-parsing crate is wired in) — this is confirmed by reading that code, not
assumed. Launching Desktop today therefore opens it to whatever it always
opens to, ignoring `--repo`/`--commit`. This is a real, open gap on the
**Desktop** side (tracked against EPIC-12/US-056, which has not shipped
yet), not a failure of this extension's own command: the moment Desktop
reads those arguments, this mechanism works with no VS Code-side change.
Until then, a missing/unconfigured/not-found Desktop executable is handled
without throwing, always offering "Copy commit hash" as a useful fallback
(US-077 criterion 3).

### Known limitations

- **Blame decorations and a dirty (unsaved) buffer**: `gitsail-cli` only
  ever reads what is saved on disk. A dirty document's blame decorations
  still render (from the last-saved content), each with an explicit
  disk-vs-buffer disclaimer in its hover — they are never silently
  presented as describing the in-editor buffer, but line numbers can still
  disagree with what is on screen if the unsaved edit inserted/removed
  lines above the decorated one.
- **Line history and a dirty buffer** (US-075 criterion 3): the same
  caveat applies to the selection's line numbers sent to
  `gitsail line-history` — `lineHistoryService.ts`'s
  `describeLineHistoryBufferCaveat` surfaces this as an explicit warning
  rather than silently querying a possibly-shifted range.
- **File rename display** (US-074 criterion 3): `gitsail log --path`
  already follows renames (US-018's existing default), so a renamed file's
  full history is returned; the rename itself becomes visible when a
  specific commit's diff is opened (`FileDiffDto.previousPath`), not as a
  separate annotation in the history list itself.

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

**Why:** at the time ADR-015 was accepted, EPIC-25 (Distribution & Updates)
had not shipped any release pipeline at all. As of T-257 (US-124), a real
pipeline now exists (`.github/workflows/release.yml`, ADR-023) and produces
checksummed CLI/TUI archives per OS — but those archives are still
**unsigned** (ADR-023 is an explicit "GitHub Releases only, no code signing
yet" decision), and this extension's own `.vsix` is built and versioned by
a separate job in that same pipeline, not bundled together with the CLI.
Embedding an unsigned CLI binary inside the extension package would still
undermine US-070 criterion 3 ("origin/version are verifiable; no silent
download/execution of an untrusted file") in spirit — the extension would
be trusting a binary bundled at `.vsix`-build time rather than one the user
consciously installed and can verify (checksum, or a future signature)
independently. Requiring an explicit, user-controlled install — from this
same GitHub Release's CLI/TUI archive, or built from source — keeps that
verification meaningful.

The full writeup lives in `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`,
**ADR-015 — VS Code binary distribution for v0.4** and **ADR-023 — GitHub
Releases-only distribution**. Revisit this decision once a code-signing
pipeline exists for the CLI binary specifically.

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
the in-editor buffer. EPIC-15 attaches that same fact to concrete UI: blame
decoration hovers (`blameFormat.ts`) and the line-history command
(`lineHistoryService.ts`) both surface it as an explicit disclaimer instead
of a silent, possibly-misleading result — see "Known limitations" above.

## `gitsail-cli` commands this extension relies on (EPIC-15)

EPIC-15 added four `gitsail-cli` subcommands the Core did not expose before
(their underlying application use cases — `GetCommit`, `GetCommitDiff`,
`GetLineHistory` — already existed; only the CLI/protocol surface was
missing), plus a brand-new Core capability (`file_content`) added
specifically for this epic:

- `gitsail commit <revision>` — a single commit's full details (US-073).
- `gitsail commit-diff <revision>` — a commit's diff against its correctly
  resolved base (root commit → empty tree, merge → first parent), never
  re-derived on the extension side (US-076 criterion 1).
- `gitsail line-history <file> --range START-END [--revision REV]` —
  commit-level history of a line range (US-075).
- `gitsail show-file <file> --revision REV` — a file's full content as of a
  revision, returning `{kind: "text"|"binary"|"missing", ...}` — backs the
  read-only historical documents above (US-076 criterion 3).

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
