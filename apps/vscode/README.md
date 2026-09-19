# GitSail for VS Code

EPIC-14 — VS Code Foundation (US-068–US-071) plus EPIC-15 — Blame & History
(US-072–US-077). This extension gives VS Code repository context, inline
blame, and commit/file/line history by reading the `git` you already have
installed — like GitLens does. There is no GitSail binary to install and
nothing to configure: install the `.vsix` and it works (ADR-025, which
supersedes ADR-015).

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

**Requirements:** `git` on the `PATH` VS Code sees, and a trusted
workspace. Nothing else — in particular, **no `gitsail` binary**. (Before
ADR-025 this extension required a separately installed `gitsail` CLI and a
`gitsail.binaryPath` setting; both are gone.)

## Architecture

```
VS Code Extension (TypeScript)
          |
   src/git/              (seven read-only queries; produces the DTOs in
          |                src/dto.ts, identical to gitsail-protocol's)
   src/git/process.ts    (the ONE module allowed to spawn `git`:
          |                no shell, argv arrays, timeout, cancellation,
          |                capped output, redacted stderr)
        git
```

ADR-025 moved this boundary. The extension used to spawn `gitsail-cli` and
parse its JSON envelope; it now reads `git` itself. The DTOs did not change,
which is why every presentation module below is untouched — and
`test/gitParity.test.ts` runs the real `gitsail` CLI side by side with
`src/git/` and requires identical DTOs, so this second implementation
cannot drift from the Rust core unnoticed. That drift risk, and the bounds
that make it acceptable, are stated in full in ADR-025's Consequences.

Module map (see each file's own doc comment for the acceptance criterion it
implements):

| File | Story | Responsibility |
|---|---|---|
| `src/dto.ts` | US-069 | The DTO shapes every presentation module consumes — field-for-field identical to `gitsail-protocol`'s. |
| `src/git/process.ts` | ADR-025 | **The one module that spawns `git`.** No shell, argv arrays, timeout, `AbortSignal` cancellation, 8 MiB output cap, redacted stderr. Allowlisted by name in `scripts/ci/check-architecture.sh`. |
| `src/git/gitClient.ts` | ADR-025 | The seven read-only queries: argument assembly (`--` before paths, `--end-of-options` before revisions) plus DTO construction. |
| `src/git/parseCommit.ts` | ADR-025 | `git log`/`git show -s` record parsing (`%x1f`/`%x1e`-delimited), timestamps, decorations. Pure. |
| `src/git/parseBlame.ts` | ADR-025 | `git blame --porcelain` parsing, including the repeated-header/omitted-metadata trap. Pure. |
| `src/git/parseDiff.ts` | ADR-025 | Unified-patch parsing into `DiffDto`, with a per-file hunk cap. Pure. |
| `src/git/redact.ts` | ADR-025 | TypeScript port of `gitsail_domain::redact` — keeps a credential-bearing URL in `git`'s stderr out of any message or log. Pure. |
| `src/git/errors.ts` / `src/git/result.ts` | ADR-025 | Typed failure hierarchy and the `GitResult` wrapper; a cancelled query is never reported as a failure. |
| `src/repositoryContext.ts` | US-068 | Associates the active file with its repository via `git rev-parse`. |
| `src/workspaceTrust.ts` | US-071 | Wraps `vscode.workspace.isTrusted`/`onDidGrantWorkspaceTrust`. |
| `src/documentState.ts` | US-071 | Classifies unsaved-buffer vs. on-disk state. |
| `src/hostTypes.ts` / `src/controller.ts` | all four | The testable orchestration layer, decoupled from the real `vscode` module. |
| `src/extension.ts` | — | The one file that imports the real `vscode` module; adapts it to `hostTypes.ts`/`historyHostTypes.ts` and hands them to `controller.ts`/`historyController.ts`. |

### EPIC-15 (US-072–US-077)

| File | Story | Responsibility |
|---|---|---|
| `src/blameFormat.ts` | US-072 | Config parsing (`gitsail.blame.*`) and pure text formatting for one blame line — never invents an author for an uncommitted line or a dirty buffer. |
| `src/blamePlan.ts` | US-072 | Decides which lines get a decoration (current-line vs. all-visible-lines) from a full `BlameDto`. |
| `src/blameService.ts` | US-072 | Blame wrapper plus client-side caches (blame per content-version, commit subjects by hash), and the cancellation that kills an abandoned debounced query. |
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

No configuration is required. The extension's own settings are
`gitsail.blame.*` (inline blame appearance and behavior) and
`gitsail.desktop.path` (the optional "Open in GitSail Desktop" handoff).

`gitsail.binaryPath` **no longer exists** (ADR-025). If you still have it in
a `settings.json` from an earlier version, VS Code will flag it as an
unknown setting; it is safe to delete.

## Requirements (ADR-025, superseding ADR-015)

Just `git`, on the `PATH` the VS Code process sees. The extension runs
`git --version` once to confirm that, and reports a single, actionable
message if it cannot — it never falls back to some other way of reading a
repository.

Before ADR-025, this section explained why the extension deliberately did
not bundle a `gitsail-cli` binary and required you to install one yourself.
That arrangement is gone: a read-only editor integration does not need a
GitSail binary, and requiring one made a freshly installed `.vsix` do
nothing but report that `gitsail.exe` was not found. Bundling a
per-platform binary instead was considered and declined — see ADR-025 for
the full reasoning, including the drift risk this decision accepts in
exchange.

## Workspace trust (US-071 criterion 2)

US-071 also depends on US-110 (EPIC-22 — Security & Safety), which has no
corresponding epic/task in the board yet. Rather than block on that, this
extension implements criterion 2 directly against VS Code's own native
workspace trust API.

**ADR-025 strengthened this gate.** Previously only a configured
`gitsail.binaryPath` was trust-gated, since that setting could arrive from a
`.vscode/settings.json` someone else committed; PATH-based discovery still
ran. With that setting gone, the workspace-controlled input that remains is
the repository itself — and running `git` in a repository honors that
repository's own configuration. So the extension now runs **no Git query at
all** until the workspace is trusted, surfacing one clear notice instead.
That is what `package.json`'s `capabilities.untrustedWorkspaces`
description always promised users, and now actually does. If/when US-110
defines a broader security policy, `src/workspaceTrust.ts` is the one place
that would grow to also honor it.

## Unsaved buffers (US-071 criterion 3)

`git` only ever reads what is actually saved on disk. `src/documentState.ts` classifies the active document's dirty/saved state, and the
controller logs an explicit note whenever the active file has unsaved
changes, instead of silently presenting results as if they described
the in-editor buffer. EPIC-15 attaches that same fact to concrete UI: blame
decoration hovers (`blameFormat.ts`) and the line-history command
(`lineHistoryService.ts`) both surface it as an explicit disclaimer instead
of a silent, possibly-misleading result — see "Known limitations" above.

## The seven Git queries this extension makes (ADR-025)

All read-only. Every one lives in `src/git/gitClient.ts`; nothing else in
the package runs a process.

| Query | Plumbing | Notes |
|---|---|---|
| Repository discovery | `git rev-parse --is-bare-repository --absolute-git-dir`, then `--show-toplevel` | Two calls, because `--show-toplevel` fails outright in a bare repository. |
| Blame | `git blame --porcelain` | An all-zero hash means an uncommitted line, reported as `origin: "local"` — never given a fabricated author. |
| File history | `git log --follow --pretty=format:…` | `%x1f`/`%x1e` separators that cannot occur in commit data; offset cursor, same as `gitsail log`'s. |
| Single commit | `git show -s --format=…` | Same format string as the file-history query. |
| Commit diff | `git diff <base> <target> -M -U3 --no-ext-diff` | `git show --patch` would print *nothing* for a merge; diffing the resolved first parent is the only form that implements the root→empty-tree / merge→first-parent policy. |
| Line history | `git log -L <start>,<end>:<file>` | The revision is resolved to a real commit up front, so the result names what it actually traced. |
| File at a revision | `git show <rev>:<path>` | Returns `{kind: "text"｜"binary"｜"missing"}`; binary and missing are values, never errors. |

The corresponding `gitsail-cli` subcommands (`gitsail commit`,
`commit-diff`, `line-history`, `show-file`) still exist and are still the
Core's own surface — this extension simply no longer calls them.

## Testing

```bash
npm install
npm run lint    # tsc --noEmit
npm run build   # tsc -p ./
npm test        # vitest run — unit + contract tests, no real vscode module
```

All business logic is covered by Vitest. Since ADR-025 the data-access
suites run against **real temporary Git repositories** built by
`test/support/tempRepo.ts` — never a mock `git`. That is a deliberate
change of approach: a stub could faithfully reproduce a JSON envelope the
CLI produced, but it can only ever emit the `git` output its author already
believed `git` emits, and the bugs worth catching here are exactly the
cases where that belief is wrong. This mirrors what `gitsail-test-support`
already does on the Rust side.

- `test/gitProcess.test.ts` — the process boundary: no shell, timeout,
  cancellation, capped output, redacted stderr.
- `test/gitClient.test.ts` — all seven queries against real repositories,
  including bare/unborn/detached repositories, renames, merges, binary
  files and uncommitted blame lines.
- `test/gitParity.test.ts` — builds the real `gitsail` CLI and requires it
  and `src/git/` to produce identical DTOs. Skips itself where `cargo` is
  absent, matching the Node-only `vscode` CI job.
- The controller suites keep using in-memory doubles, since they are about
  orchestration (debounce, generation guards, quick-pick flow) rather than
  about Git.

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
