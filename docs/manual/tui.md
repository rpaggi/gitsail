# GitSail TUI manual

The terminal interface (`crates/gitsail-tui`, ADR-005/Ratatui, SAD §18/§37).
Everything below is verified directly against `crates/gitsail-tui/src/{main,keymap,action,ui,keybindings}.rs`
as of this writing (2026-09-18) — no command or shortcut here is aspirational.
See [`docs/manual/troubleshooting.md`](./troubleshooting.md) for credentials,
configuration file locations, updates, privacy, and protocol compatibility
(none of that is TUI-specific).

## Opening and navigating

The friendliest way to open it, once the CLI is installed, is simply:

```sh
gitsail
```

`gitsail` with no subcommand (or the explicit `gitsail tui`) opens the exact
same interactive interface as the separate `gitsail-tui` binary — a common
convention for a Git terminal client (`lazygit`/`tig` do the same), so there
is nothing extra to install or remember. Every read-only query still works
exactly as before (`gitsail status`, `gitsail log`, ...); only the *absence*
of a subcommand changed meaning, from a usage error to opening the TUI. Both
entry points share one implementation (`gitsail_tui::run_interactive`, see
`crates/gitsail-tui/src/runtime.rs`) — they can never drift apart.

The standalone binary, and building/running from source, still work exactly
as before:

```sh
gitsail-tui --repo /path/to/repository
# or, from a source checkout, without installing anything:
cargo run -p gitsail-tui --release -- --repo /path/to/repository
```

Flags (shared by both entry points — `crates/gitsail-tui/src/main.rs` for the
standalone binary, `crates/gitsail-cli/src/cli.rs` for `gitsail`/`gitsail tui`):

| Flag | Meaning |
|---|---|
| `--repo <path>` | Repository to open (default: current directory). |
| `--git-path <path>` | Explicit `git` executable (default: `git` on `PATH`). |
| `--ascii` | Disable color, and swap the decorative glyphs (panel icons, branch dots, selection cursors, keycap boxes, box corners, the GitSail mark) for ASCII equivalents. Every state the colored interface shows — focus, selection, current branch, local vs. remote, staged vs. unstaged, added vs. removed diff lines, clean vs. dirty — stays distinguishable, because each one is carried by a marker or a glyph *shape* as well as by its color. The commit graph's own lane glyphs (`●`/`○`/`◆`/`│`) are unchanged in this mode, as they always have been. Also enabled automatically when the `NO_COLOR` environment variable is set. |
| `--keybindings <path>` | Explicit keybindings-override file (default: `<OS config dir>/gitsail/tui/keybindings.conf` — see [keyboard remapping](#keyboard-remapping) below). |

The terminal needs a minimum size — 78x20 — below which the TUI shows
"Terminal too small (WxH). Resize to at least 78x20." instead of a broken
layout. Between that floor and a full-size window the layout adapts by
narrowing its side columns, never by hiding a panel.

**Layout**: a left chrome column (the GitSail mark, a navigation menu
mirroring the current focus, the current branch and working-tree state, and
a help strip), a wide center column (**Commits** above, **Diff**/**Blame**
below with the **Changes** file list nested inside it), a right column
(**Repository** facts, **Branches**, and the current **References**
sub-view), and a keycap strip along the bottom. The navigation menu on the
left is a read-out of where focus currently is, not a separate control: it
follows `Tab`, `t` and `b` rather than being driven on its own.

**Panels** (five, unchanged): Branches, Commits, Changes, Diff/Blame, and
References (tags/remotes/stash/reflog, read-only). `Tab` /
`Shift+Tab` move focus between panels; `Up`/`Down` or `j`/`k` move the
selection within the focused panel (the focused panel is the one whose box
is marked `»` in its title); `Enter` activates/opens whatever is selected; `?` toggles a contextual help overlay listing every shortcut below;
`q` or `Ctrl+C` quits (from the Normal context — `q` inside an overlay closes
that overlay instead, never the whole app, and `r` refreshes status/branches
manually).

## Day-to-day operations

| Shortcut | Where | Action |
|---|---|---|
| `s` | Changes panel | Stage/unstage the highlighted entry. |
| `C` | anywhere (Normal) | Open the commit-message composer; `Enter` confirms, `Esc` cancels. |
| `y` | Diff panel | Copy the diff's patch (falls back to saving a file if the clipboard is unavailable). |
| `Y` | Diff panel | Preview-apply the patch currently on the clipboard (`git apply --check`); confirming the prompt actually applies it. |
| `n` | Branches panel | Create a branch (name prompt). |
| `c` | Branches panel | Checkout the highlighted branch (confirmation). |
| `d` | Branches panel | Delete the highlighted branch (confirmation). |
| `R` | Branches panel | Rename the highlighted branch, pre-filled with its current name. |
| `f` | anywhere (Normal) | Fetch the resolved remote (Safe — no confirmation). |
| `p` | anywhere (Normal) | Pull the tracked branch, fast-forward only (confirmation). |
| `P` | anywhere (Normal) | Push the current branch (confirmation). |
| `b` | Changes/Diff panel | Toggle between the diff and blame sub-views. |
| `/` | Branches panel | Filter the branch list. |
| `/` | Commits panel | Search commits (`text`, `author:`, `branch:`, `hash`). |
| `t` | References panel | Cycle Tags / Remotes / Stash / Reflog sub-views (read-only listing — see [Limitations](#limitations)). |
| `Enter` | References panel | View the highlighted entry's details (a reflog entry opens its commit's details, when that commit still exists). |
| `w` | anywhere (Normal) | Open the current selection (branch/commit/repo root) on its detected GitHub/GitLab remote's web page. A no-op when no configured remote resolves to a known forge. |

## Advanced operations

| Shortcut | Where | Action |
|---|---|---|
| `m` | Branches panel | Merge the highlighted reference into the current branch (confirmation shows origin, destination, policy). |
| `o` | Branches panel | Rebase the current branch onto the highlighted reference (confirmation). |
| `O` | Branches panel | Open the interactive rebase plan for the highlighted reference (read-only until confirmed). Inside the plan: `j`/`k` select an entry, `J`/`K` reorder it, `a` cycles its action Pick → Reword → Squash → Fixup → Drop → Pick, `Enter` confirms the plan. |
| `x` | Commits panel | Cherry-pick the highlighted commit onto the current branch (confirmation). A merge commit always uses the fixed first-parent policy, named explicitly in the confirmation. |
| `v` | Commits panel | Revert the highlighted commit (confirmation). |
| `z` | Commits panel | Open the reset-mode chooser for the highlighted commit — pick soft/mixed/hard, then confirm. |
| `A` | anywhere (Normal) | Open the amend composer, pre-loaded with HEAD's current message and staged diff; edit the message, `Enter` amends (confirmation), `Esc` discards (a failed amend keeps the typed message and never silently refreshes). |

Reset modes (identical semantics in Desktop, see `docs/manual/desktop.md`):
**soft** (HEAD moves only; index/working tree preserved), **mixed** (HEAD and
index move; working tree preserved), **hard** (HEAD, index, and working tree
all move — uncommitted changes are discarded; this is the `Destructive`
mutation the reinforced-confirmation policy applies to, see below).

## Conflicts and recovery

`M` opens or closes the Conflicts overlay — a no-op when no operation with
conflicts is currently pending. Inside it:

| Key | Action |
|---|---|
| `j`/`k` or `Up`/`Down` | Move between conflicted files. |
| `Enter` | Inspect the highlighted file (loads base/ours/theirs). |
| `r` | Mark the highlighted file resolved by staging its current working-tree content. |
| `o` | Resolve by taking "ours" wholesale. |
| `t` | Resolve by taking "theirs" wholesale. |
| `c` | Request confirmation to **continue** the pending operation — only offered when the in-progress operation (merge, rebase, cherry-pick, revert) actually supports `Continue`. |
| `a` | Request confirmation to **abort** the pending operation — only offered when it supports `Abort`. |
| `s` | Request confirmation to **skip** the current step — only offered when it supports `Skip` (a plain merge never offers it; rebase does). |
| `Esc` / `q` | Close the overlay (does not itself abort anything). |

This is the same conflict lifecycle for merge, rebase, cherry-pick and
revert: a conflicting operation always leaves the repository in a named,
inspectable state; continue/skip/abort are always explicit, confirmed
actions, never inferred. Declining any pending confirmation anywhere in the
TUI (pressing `Esc`/closing the dialog before confirming) leaves the
repository completely untouched — this is a structural guarantee, not a
convention: the confirmation step for a `Moderate`/`Destructive` mutation can
never be skipped or remapped away (see `docs/architecture/preferences-matrix.md`'s
"confirmation-policy invariant" section for the proof).

## Keyboard remapping

Ten common Normal-context shortcuts can be remapped via a plain-text config
file (`quit`, `toggle-help`, `refresh`, `toggle-stage`, `start-commit`,
`request-fetch`, `request-pull`, `request-push`, `toggle-blame-view`,
`start-search`) — default location `<OS config dir>/gitsail/tui/keybindings.conf`
(override with `--keybindings <path>`), one `action-id = X` assignment per
line (`X` a single character), `#` for comments. An unknown id or an invalid
value is reported as a warning on stderr at startup and ignored — the file
never crashes the TUI, it just falls back to that action's default. This is
deliberately narrow: no Vim/Emacs modal editing, no per-motion remapping, no
macro system, and `Activate` (`Enter`)/`Dismiss` (`Esc`) can never be
remapped — every overlay/confirmation context always resolves through the
original, hardcoded keymap regardless of what the config file contains.

## Limitations

- **Tags, remotes, stash and reflog are read-only** in the References panel
  — there is no TUI action to create/apply/pop/drop a stash, or to
  create/delete/annotate a tag. Mutating these is defined and tested at the
  Core layer (`gitsail-application`/`gitsail-git`) but has no TUI wiring yet.
- **Worktrees** are not exposed at all (no listing, no create/remove).
- **Force-push (with lease)** is deliberately excluded from the Push action
  — a rejected push is never silently escalated to a force push.
- **Mid-flight cancellation** of a running operation is not implemented —
  the TUI's only "cancel" semantics is declining a confirmation *before* it
  starts (see above); once a mutation is running, it runs to completion.
- **Pull/Merge Request listing** is not wired into the TUI (Desktop only).
