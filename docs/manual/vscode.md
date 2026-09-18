# GitSail for VS Code — manual

The VS Code extension (`apps/vscode`, EPIC-14/EPIC-15). It gives repository
context, inline blame, and commit/file/line history entirely by asking a
separately-installed `gitsail` CLI process — it never re-implements Git
detection, diffing, or blame parsing itself (ADR-007). Everything below is
verified directly against `apps/vscode/package.json`'s
`contributes.commands`/`configuration` and `apps/vscode/README.md` as of this
writing (2026-09-18) — no command or setting here is aspirational. See
[`docs/manual/troubleshooting.md`](./troubleshooting.md) for credentials,
updates, privacy, and protocol compatibility (none of that is VS Code-specific).

**This extension is read-only.** It has no stage/commit/branch/sync/merge/
rebase/conflict UI at all — see [Limitations](#limitations). It is a
companion to a repository you edit and commit with something else (a
terminal, the TUI, Desktop, or VS Code's own built-in Git support), not a
replacement for any of those.

## Opening and navigating

1. Install and build a `gitsail` binary that satisfies this extension's
   minimum supported CLI version (`MINIMUM_SUPPORTED_CLI_VERSION` in
   `src/cliLocator.ts`), and put it on `PATH`, or set `gitsail.binaryPath` to
   its absolute path.
2. Open a folder inside a Git repository in VS Code, in a **trusted**
   workspace — an untrusted workspace never reads `gitsail.binaryPath` and
   the extension surfaces a one-time notice instead of silently ignoring it.
3. The extension activates on startup and associates the active file with
   its repository automatically; no explicit "open repository" step exists.

Every command below is registered under the "GitSail" category in the
Command Palette (`Ctrl/Cmd+Shift+P`), and the three marked "editor context
menu" also appear when you right-click inside a file editor.

## Commands

| Command | Where | What it does |
|---|---|---|
| **Toggle Inline Blame** (`gitsail.blame.toggle`) | Palette, editor context menu | Turns inline blame decorations on/off for the current editor. |
| **Open Commit Details** (`gitsail.openCommitDetails`) | Palette | Opens a read-only virtual document with one commit's full details (hash, author, date, subject, body) — served through a `gitsail-commit:` URI, never written to disk. |
| **Copy Commit Hash** (`gitsail.copyCommitHash`) | Palette | Copies a commit's full hash to the clipboard. |
| **Show File History** (`gitsail.showFileHistory`) | Palette, editor context menu (any file) | Opens a Quick Pick of the current file's commit history (follows renames). |
| **Show Line History** (`gitsail.showLineHistory`) | Palette, editor context menu (requires a text selection) | Opens a Quick Pick of the commit history for the selected line range. |
| **Open Commit in GitSail Desktop** (`gitsail.openInDesktop`) | Palette | Launches the configured GitSail Desktop executable with `--repo`/`--commit`, or offers "Copy commit hash" as a fallback when Desktop isn't configured/found (see [Limitations](#limitations) — Desktop doesn't act on these arguments yet). |

Selecting a commit from the file/line history Quick Pick, or opening commit
details, lets you further open that revision's diff or its full historical
file content (`gitsail-history:` URI) — both read-only virtual documents,
never a temp file on disk and never an editable buffer.

## Configuration

All settings are under `gitsail.*` in VS Code's own Settings UI
(`Ctrl/Cmd+,`, search "GitSail") or `settings.json` — see
`docs/manual/troubleshooting.md`'s "Configuration files" section for exactly
where VS Code itself stores that file per platform.

| Setting | Default | Meaning |
|---|---|---|
| `gitsail.binaryPath` | `""` (discover on `PATH`) | Absolute path to the `gitsail` executable. Trusted workspaces only. |
| `gitsail.blame.enabled` | `true` | Show inline blame decorations at all. |
| `gitsail.blame.mode` | `"currentLine"` | `"currentLine"` (only the cursor's line) or `"allVisibleLines"`. |
| `gitsail.blame.delayMs` | `400` | Milliseconds of cursor inactivity before a decoration (re)appears. A negative value falls back to `400`. |
| `gitsail.blame.format` | `"${author}, ${date} • ${shortHash} • ${message}"` | Template; placeholders `${author}`, `${authorEmail}`, `${date}`, `${hash}`, `${shortHash}`, `${message}`. |
| `gitsail.blame.dateStyle` | `"absolute"` | `"absolute"` (`2024-03-02`) or `"relative"` (`"3 days ago"`); an unrecognized value falls back to `"absolute"`. |
| `gitsail.desktop.path` | `""` | Absolute path to the GitSail Desktop executable, for "Open Commit in GitSail Desktop". Empty disables the handoff. |

## Conflicts and recovery

**Not applicable.** This extension never mutates the repository, so it has
no merge/rebase/conflict/continue/abort UI to document — see
[Limitations](#limitations). Resolve a conflict with the TUI, Desktop, or
another Git tool, then use this extension's read-only views (blame,
history, commit details) as normal once the repository is back in a clean
state.

## Known limitations

- **Read-only only**: no stage/commit/branch/merge/rebase/cherry-pick/
  revert/reset/stash/tag/worktree/sync command exists in this extension at
  all. Use the TUI or Desktop for any of those, or VS Code's own built-in
  Git support.
- **Dirty (unsaved) buffer**: `gitsail` only reads what is saved on disk. A
  dirty document's blame decorations still render from the last-saved
  content with an explicit disk-vs-buffer disclaimer in the hover — never
  silently presented as describing the in-editor buffer — but line numbers
  can disagree with the screen if an unsaved edit inserted/removed lines
  above the decorated one. The same caveat applies to Show Line History's
  selected range.
- **No binary bundled**: this extension does not ship a `gitsail` binary in
  its `.vsix` (a deliberate v0.4 decision, ADR-015) — you must install one
  yourself and either put it on `PATH` or set `gitsail.binaryPath`. This
  will be revisited once EPIC-25 (Distribution & Updates) ships a signed,
  verifiable per-platform artifact (see `troubleshooting.md`'s "Updates"
  section — EPIC-25 has not shipped yet).
- **"Open Commit in GitSail Desktop" doesn't yet select the commit/repo**:
  Desktop does not parse command-line arguments yet, so this always opens
  Desktop to whatever it would open to anyway. This is a documented gap on
  the Desktop side, not this extension's.
- **No `AbortSignal` plumbed into blame/history CLI calls**: a long-running
  blame/history query cannot currently be cancelled mid-flight from this
  extension, even though the underlying CLI client supports it — an
  already-escalated, known gap (not attempted as a drive-by fix in this
  documentation task).
