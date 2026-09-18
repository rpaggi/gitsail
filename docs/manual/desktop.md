# GitSail Desktop manual

The Tauri v2 + Vue 3 desktop application (`apps/desktop`, ADR-006, SAD §17).
Everything below is verified directly against `apps/desktop/src/components/*.vue`
and `apps/desktop/src/stores/*.ts` as of this writing (2026-09-18) — no button,
menu item, or shortcut here is aspirational. See
[`docs/manual/troubleshooting.md`](./troubleshooting.md) for credentials,
configuration file locations, updates, privacy, and protocol compatibility
(none of that is Desktop-specific).

## Opening and navigating

From `apps/desktop`:

```sh
npm install
npm run tauri dev   # windowed dev build; needs a Linux display + WebKitGTK, or Windows/macOS
```

No packaged installer exists yet for any OS (see
[Limitations](#limitations) and `troubleshooting.md`'s "Updates" section) —
Desktop is currently run from source.

**Layout** (`AppShell.vue`): a header with the GitSail identity and the
current repository/branch; a **sidebar** with repository selection (open a
folder or pick a recent repository), theme and keyboard-shortcut settings,
search, branches, sync (remotes/fetch/pull/push), and pull/merge request
listing; a **main area** with the commit graph on top and three tabs below
it — **Changes** (staging + diff viewer), **Merge & Rebase**, and **Amend**.
Tabs stay mounted when you switch between them (`v-show`, not `v-if`), so an
in-progress commit message or an open rebase plan is never discarded by
switching tabs.

## Day-to-day operations

- **Open a repository**: type or "Browse…" a path in the sidebar's
  "Repository" section, then "Open". Previously opened repositories appear
  under "Recent repositories"; an entry that no longer resolves to a valid
  repository shows its error inline with a "Remove from list" action.
- **Stage / unstage**: the Changes tab's Staging panel lists unstaged and
  staged files side by side, each with its own Stage/Unstage button; files
  can also be dragged between the two columns. Clicking a file's path opens
  its diff in the diff viewer below.
- **Commit**: type a message in the composer and click "Commit N file(s)"
  (disabled until at least one file is staged and the message is non-empty).
- **View a diff**: the diff viewer toggles between "Unified" and
  "Side-by-side" without re-fetching; a binary or truncated file shows an
  explicit banner instead of fabricated content.
- **Copy a patch**: in the status list, "Copy staged patch" / "Copy patch"
  copies that file's patch to the clipboard (falls back to saving a file if
  the clipboard is unavailable, and reports when the diff was empty or the
  copy/save failed).
- **Apply a patch**: paste text or "Choose file…" in the "Apply a patch"
  panel, "Preview" (a non-mutating `git apply --check`) shows which files it
  affects or why it was rejected, then "Apply patch" actually applies it.
- **Branches**: the Branches list offers Switch (non-current branches),
  Rename (any branch, pre-filled with its current name), Delete
  (non-current branches), and a "Create" field for a new branch name.
- **Sync**: the Sync panel shows the resolved fetch/pull/push target
  (branch ↔ remote) and Fetch / Pull (fast-forward only) / Push buttons; an
  "Open in browser" button appears only once a configured remote resolves to
  a recognized GitHub/GitLab forge.
- **Pull/Merge Requests**: read-only listing (title, state, author,
  source/target branch) with explicit states for loading, no forge
  detected, no connected/authorized token, rate-limited, offline, and error
  — never conflated with "no results". Each item has an explicit "Open in
  browser" click; nothing opens automatically.
- **Search**: the sidebar's search box (default shortcut `Ctrl/Cmd+K` to
  focus it — see [Keyboard shortcuts](#keyboard-shortcuts)) finds commits by
  hash/message/author and branches by name; selecting a commit result
  selects the same commit in the graph. A selected commit result offers
  "Copy hash" and "Branch here…".
- **Theme**: Dark/Light toggle buttons in the sidebar's Settings section.

## Advanced operations

- **Merge** (Merge & Rebase tab): choose a reference from the dropdown,
  "Merge into current". The result banner distinguishes fast-forward, a
  created merge commit, or a conflict.
- **Rebase**: choose a base, "Rebase current onto…" for a plain rebase, or
  "Plan interactive rebase…" to open the reorderable plan (see below).
- **Interactive rebase plan**: lists the candidate commits onto the chosen
  base; each entry has ↑/↓ buttons to reorder it and a dropdown to choose
  its action (pick / reword / squash / fixup / drop) — a "reword" entry gets
  its own message textarea. "Confirm plan" runs it through the same
  confirm/result flow as every other mutation; "Cancel" discards the plan
  without touching the repository.
- **Cherry-pick / Revert / Reset**: right-click a commit in the commit graph
  (or press the keyboard context-menu key/`Shift+F10` on the selected row)
  for "Cherry-pick commit…", "Revert commit…", "Reset to…", and "Copy hash".
  Reset opens a mode chooser: **Soft** (HEAD moves only; index and working
  tree preserved — changes become staged), **Mixed** (HEAD and index move;
  working tree preserved — changes become unstaged), or **Hard** (HEAD,
  index and working tree all move; uncommitted changes are **permanently
  discarded** — the dialog shows the live count of files that would be lost
  before you can confirm).
- **Amend** (Amend tab): "Amend last commit…" loads a read-only preview
  (HEAD's current message and the staged diff that would be folded in);
  edit the message and "Amend HEAD" to confirm.

## Conflicts and recovery

A merge, rebase, cherry-pick, or revert that produces a conflict shows a
distinct result banner ("CONFLICT — N file(s) need resolution below") and
lists the conflicted files in the Merge & Rebase tab, each labeled with its
exact conflict stage (both modified, both added, both deleted, added/deleted
by us/them). Clicking a file expands base/ours/theirs content inspection.
Per file: "Mark resolved" (stage its current working-tree content), "Take
ours", or "Take theirs". Once ready, the same panel's action row offers
**Continue**, **Skip**, and **Abort** — each disabled unless the
currently-pending operation actually supports that capability (e.g. a plain
merge never offers Skip). A cherry-pick that turns out to already be applied
reports "EMPTY" rather than a false conflict, with the same
Continue/Skip/Abort recovery path.

Every mutation in this app goes through one shared confirmation dialog
(risk badge Safe/Moderate/Destructive, the exact target, and — for a
destructive one — the concrete impact) before anything runs; pressing
Cancel or `Esc` leaves the repository completely untouched. This is
structural, not a convention some button could bypass: a keyboard remap
(below) can only ever change which key *starts* one of five fixed, already
Safe/Moderate actions (focus-search, fetch, pull, push, commit) — never
reach a destructive operation directly.

## Keyboard shortcuts

Five global actions are remappable in the sidebar's "Keyboard shortcuts"
panel (each binding must include `Ctrl` or `Cmd` — a bare letter/Shift/Alt
combination can never be captured, so remapping can never break ordinary
typing in a text field):

| Action | Default binding |
|---|---|
| Focus search | `Ctrl/Cmd+K` |
| Fetch | `Ctrl/Cmd+Shift+F` |
| Pull | `Ctrl/Cmd+Shift+L` |
| Push | `Ctrl/Cmd+Shift+P` |
| Create commit | `Ctrl/Cmd+Enter` |

"Remap" captures the next key combination you press; "Reset" restores that
one action's default; "Restore all defaults" resets every action. A
conflict (two actions sharing one binding) is flagged inline; the first
action in registration order wins if both bindings are ever pressed. All
five actions call an already-existing, already-safe/moderate store action —
this registry can never be extended to reach a destructive operation without
an explicit code change (enforced by a dedicated test,
`keybindings.test.ts`).

## Limitations

- **No tags or stash panel exists yet** in Desktop — not even read-only
  listing (confirmed by reading every component under `apps/desktop/src/components/`;
  `AppShell.vue`'s own module doc records the same gap). Tags/stash
  mutation is implemented and tested at the Core layer but has no UI in any
  interface; querying them from Desktop specifically is tracked as its own,
  not-yet-implemented backlog item (US-062).
- **Worktrees** are not exposed at all.
- **Force-push (with lease)** is explicitly out of scope by design — a
  rejected push is never silently escalated to a force push.
- **No blame view** exists in Desktop today (VS Code has inline blame; the
  TUI has a dedicated blame panel).
- **No UI to connect/disconnect a forge (GitHub/GitLab) account.** The
  backend commands exist (`connect_forge_account`, `disconnect_forge_account`,
  `forge_connection_status` in `apps/desktop/src-tauri/src/commands.rs`) and
  are tested there, but no Vue component calls them (confirmed by grepping
  every file under `apps/desktop/src/components` and `src/stores`) — the
  Pull/Merge Requests panel's "no token" state only offers "Retry", not
  "Connect". Until a connect-account UI exists, listing pull/merge requests
  for a private repository is not reachable from this app's own interface.
- **No mid-flight cancellation** of a running operation — Desktop's only
  "cancel" semantics is declining the confirmation dialog before a mutation
  starts.
- **Launching Desktop from VS Code's "Open Commit in GitSail Desktop"
  command currently ignores the commit/repository it was asked to open** —
  Desktop does not parse any command-line arguments yet (confirmed by
  reading `apps/desktop/src-tauri/src/{main,lib}.rs`), so it always opens to
  whatever it would open to anyway. This is a known, already-documented gap
  on the Desktop side (`apps/vscode/README.md`'s "Desktop handoff" section),
  not a VS Code defect.
