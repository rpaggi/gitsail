# GitSail TUI manual

The terminal interface (`crates/gitsail-tui`, ADR-005/Ratatui, SAD §18/§37).
Everything below is verified directly against `crates/gitsail-tui/src/{main,keymap,action,ui,keybindings}.rs`
as of this writing (2026-09-18) — no command or shortcut here is aspirational.
See [`docs/manual/troubleshooting.md`](./troubleshooting.md) for credentials,
configuration file locations, updates, privacy, and protocol compatibility
(none of that is TUI-specific).

## Opening and navigating

```sh
cargo run -p gitsail-tui --release -- --repo /path/to/repository
```

Flags (`crates/gitsail-tui/src/main.rs`):

| Flag | Meaning |
|---|---|
| `--repo <path>` | Repository to open (default: current directory). |
| `--git-path <path>` | Explicit `git` executable (default: `git` on `PATH`). |
| `--ascii` | Disable color; render with text/markers only. Also enabled automatically when the `NO_COLOR` environment variable is set. |
| `--keybindings <path>` | Explicit keybindings-override file (default: `<OS config dir>/gitsail/tui/keybindings.conf` — see [keyboard remapping](#keyboard-remapping) below). |

The terminal needs a minimum size; below it the TUI shows "Terminal too
small (WxH). Resize to at least MIN_WIDTHxMIN_HEIGHT." instead of a broken
layout.

**Panels**: Sidebar (branches), Graph (commit history), Details/Diff (status,
diff, blame), References (tags/remotes/stash/reflog, read-only). `Tab` /
`Shift+Tab` move focus between panels; `Up`/`Down` or `j`/`k` move the
selection within the focused panel; `Enter` activates/opens whatever is
selected; `?` toggles a contextual help overlay listing every shortcut below;
`q` or `Ctrl+C` quits (from the Normal context — `q` inside an overlay closes
that overlay instead, never the whole app, and `r` refreshes status/branches
manually).

## Day-to-day operations

| Shortcut | Where | Action |
|---|---|---|
| `s` | Details panel | Stage/unstage the highlighted entry. |
| `C` | anywhere (Normal) | Open the commit-message composer; `Enter` confirms, `Esc` cancels. |
| `y` | Diff panel | Copy the diff's patch (falls back to saving a file if the clipboard is unavailable). |
| `Y` | Diff panel | Preview-apply the patch currently on the clipboard (`git apply --check`); confirming the prompt actually applies it. |
| `n` | Sidebar | Create a branch (name prompt). |
| `c` | Sidebar | Checkout the highlighted branch (confirmation). |
| `d` | Sidebar | Delete the highlighted branch (confirmation). |
| `R` | Sidebar | Rename the highlighted branch, pre-filled with its current name. |
| `f` | anywhere (Normal) | Fetch the resolved remote (Safe — no confirmation). |
| `p` | anywhere (Normal) | Pull the tracked branch, fast-forward only (confirmation). |
| `P` | anywhere (Normal) | Push the current branch (confirmation). |
| `b` | Details/Diff panel | Toggle between the diff and blame sub-views. |
| `/` | Sidebar | Filter the branch list. |
| `/` | Graph panel | Search commits (`text`, `author:`, `branch:`, `hash`). |
| `t` | References panel | Cycle Tags / Remotes / Stash / Reflog sub-views (read-only listing — see [Limitations](#limitations)). |
| `Enter` | References panel | View the highlighted entry's details (a reflog entry opens its commit's details, when that commit still exists). |
| `w` | anywhere (Normal) | Open the current selection (branch/commit/repo root) on its detected GitHub/GitLab remote's web page. A no-op when no configured remote resolves to a known forge. |

## Advanced operations

| Shortcut | Where | Action |
|---|---|---|
| `m` | Sidebar | Merge the highlighted reference into the current branch (confirmation shows origin, destination, policy). |
| `o` | Sidebar | Rebase the current branch onto the highlighted reference (confirmation). |
| `O` | Sidebar | Open the interactive rebase plan for the highlighted reference (read-only until confirmed). Inside the plan: `j`/`k` select an entry, `J`/`K` reorder it, `a` cycles its action Pick → Reword → Squash → Fixup → Drop → Pick, `Enter` confirms the plan. |
| `x` | Graph panel | Cherry-pick the highlighted commit onto the current branch (confirmation). A merge commit always uses the fixed first-parent policy, named explicitly in the confirmation. |
| `v` | Graph panel | Revert the highlighted commit (confirmation). |
| `z` | Graph panel | Open the reset-mode chooser for the highlighted commit — pick soft/mixed/hard, then confirm. |
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
