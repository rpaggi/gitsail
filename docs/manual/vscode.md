# GitSail for VS Code — manual

The VS Code extension (`apps/vscode`, EPIC-14/EPIC-15). It gives repository
context, inline blame, and commit/file/line history by reading the `git` you
already have installed, the way GitLens does. **There is no GitSail binary
to install and nothing to configure** (ADR-025, which supersedes ADR-015 —
earlier versions of this extension required a separately installed `gitsail`
CLI and a `gitsail.binaryPath` setting). Everything below is
verified directly against `apps/vscode/package.json`'s
`contributes.commands`/`configuration` and `apps/vscode/README.md` as of this
writing (2026-09-19) — no command or setting here is aspirational. See
[`docs/manual/troubleshooting.md`](./troubleshooting.md) for credentials,
updates, privacy, and protocol compatibility (none of that is VS Code-specific).

**This extension is read-only.** It has no stage/commit/branch/sync/merge/
rebase/conflict UI at all — see [Limitations](#limitations). It is a
companion to a repository you edit and commit with something else (a
terminal, the TUI, Desktop, or VS Code's own built-in Git support), not a
replacement for any of those.

## Opening and navigating

1. Make sure `git` is on the `PATH` VS Code sees. That is the extension's
   only requirement; if `git` cannot be run, the extension says so once,
   with what to do about it, and stays inactive.
2. Open a folder inside a Git repository in VS Code, in a **trusted**
   workspace. In an untrusted workspace the extension runs **no Git query
   at all** and surfaces a one-time notice instead — reading a repository
   with `git` honors that repository's own configuration, so GitSail waits
   for you to trust the workspace first.
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
| `gitsail.blame.enabled` | `true` | Show inline blame decorations at all. |
| `gitsail.blame.mode` | `"currentLine"` | `"currentLine"` (only the cursor's line) or `"allVisibleLines"`. |
| `gitsail.blame.delayMs` | `400` | Milliseconds of cursor inactivity before a decoration (re)appears. A negative value falls back to `400`. |
| `gitsail.blame.format` | `"${author}, ${date} • ${shortHash} • ${message}"` | Template; placeholders `${author}`, `${authorEmail}`, `${date}`, `${hash}`, `${shortHash}`, `${message}`. |
| `gitsail.blame.dateStyle` | `"absolute"` | `"absolute"` (`2024-03-02`) or `"relative"` (`"3 days ago"`); an unrecognized value falls back to `"absolute"`. |
| `gitsail.desktop.path` | `""` | Absolute path to the GitSail Desktop executable, for "Open Commit in GitSail Desktop". Empty disables the handoff. |

There is no `gitsail.binaryPath` setting. It was removed by ADR-025 along
with the CLI dependency; if you still have it in a `settings.json` from an
earlier version, VS Code will flag it as unknown and it is safe to delete.

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
- **A second, small implementation of Git reading**: since ADR-025 this
  extension parses `git`'s output itself rather than asking the GitSail
  core, which is what lets the `.vsix` stand alone. The honest trade-off is
  that this slice *can* disagree with the CLI/TUI/Desktop, which all share
  the Rust core. It is read-only (a disagreement can show you something
  wrong; it cannot damage a repository) and it is checked against the real
  `gitsail` binary by `apps/vscode/test/gitParity.test.ts`, but that test
  only covers the cases it enumerates — an unusual patch format or encoding
  could still be handled differently by the two. Report it if you see it.
- **"Open Commit in GitSail Desktop" doesn't yet select the commit/repo**:
  Desktop does not parse command-line arguments yet, so this always opens
  Desktop to whatever it would open to anyway. This is a documented gap on
  the Desktop side, not this extension's.
- **Cancellation covers blame, not the history commands**: ADR-025 plumbed
  real cancellation into the inline-blame path, which is the one that
  matters — it is debounced and re-fired as the cursor moves, so an
  abandoned query is now actually killed rather than left running. The
  one-shot history commands (Show File History, Show Line History, commit
  details) still run to completion once started; they are user-initiated
  and bounded by a timeout, so this is a much smaller gap than it was, but
  it is not zero.
