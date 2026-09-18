# Desktop layout & accessibility — US-053 / US-055

Covers T-186 (US-053 — "Compor layout e identidade GitSail") and T-188
(US-055 — "Oferecer navegação acessível"), both EPIC-11 — Desktop
Foundation, v0.3. Written as the DoD for both stories requires: a review
against the original assets/PRD documenting visual decisions and empty/
error/loading states (US-053), and an accessibility checklist plus a
documented no-mouse flow (US-055).

Both stories were blocked on US-129 (T-262, GitSail brand identity —
delivered, commit `c96e332`) and are implemented together here because
US-055 depends directly on US-053's layout.

## 1. What changed

Before this story, `App.vue` mounted every panel (`BranchPanel`,
`SyncPanel`, `MergePanel`, `AmendPanel`, `PullRequestsPanel`,
`SearchPalette`, `CommitGraph`, `StagingPanel`, `DiffViewer`, ...) as one
flat stack with a two-column CSS flexbox as its only structure, no header
identity beyond a bare `<h1>GitSail</h1>`, and no shell-level empty/error/
loading state (each panel handled its own).

This story adds:

- `src/components/AppShell.vue` — the consolidated layout (header,
  sidebar, main with tabs). `App.vue` now only bootstraps (mount-time
  wiring: startup intent, window-focus refresh) and renders `<AppShell />`
  plus the two global modal overlays (`ConfirmationDialog`,
  `HistoryEditingPanel`).
- `src/theme.css` — CSS custom-property design tokens (color, font),
  a global visible-focus rule, a skip link, and an `.sr-only` utility.
  Imported once from `main.ts`.
- `src/components/shellState.ts` (+ test) — the pure function deciding
  which of `opening`/`empty`/`error`/`ready` the shell is in.
- `src/components/keyboardNav.ts` (+ test) — pure roving-tabindex and
  focus-trap boundary math, shared by the new tabs, the commit graph's
  keyboard navigation, and the two modal dialogs.
- `src/components/focusTrap.ts` — the (non-pure, DOM-touching) wiring
  around `keyboardNav.ts`'s trap math, shared by `ConfirmationDialog.vue`
  and `HistoryEditingPanel.vue`.
- Every existing panel component is **reused as-is** and only repositioned;
  the only per-component edits are accessibility fixes (below) — no panel
  was rewritten.

No new binary asset was created. `apps/desktop/public/branding/
logo_gitsail.png` is a byte-for-byte copy of `assets/branding/
logo_gitsail.png` (verified with the same file, not edited or
re-encoded) placed where Vite's static-file convention (`public/`) can
serve it to the header — `assets/README.md`'s "do not edit in place" rule
is about *editing* the original, which this does not do; the original
under `assets/` is untouched.

## 2. Visual decisions vs. the mockup, brand identity, and PRD

Reference: `assets/mockups/gitsail_gui_mockup.png`, `docs/product/
brand-identity.md`, `assets/README.md`.

- **Identity (US-053 criterion 2).** The header (`AppShell.vue`) shows the
  existing logo image, the "GitSail" wordmark as real text (not just baked
  into the image, so it stays legible at any header height and to a screen
  reader independent of the image's `alt` text), and the tagline
  "Navigate your Git history." verbatim. The nautical concept is present as
  a small inline SVG (mast + sail + wave, `.app-shell__sail-mark`) built
  from three `<path>` primitives colored with the new sea-blue/sail-cloth
  tokens — deliberately generic geometry, not a redrawing of the mascot
  illustration, so it reads as "a boat/sail motif" without asserting it *is*
  the mascot artwork. The overall dark theme uses a "deep sea to sail
  cloth" blue gradient (`--color-bg` through `--color-accent-strong` in
  `theme.css`) rather than an arbitrary dark gray, which is the "conceito
  náutico... de forma sutil" the criterion asks for beyond the literal logo.
- **Layout shape, not pixel copy (US-053 criterion 3).** The mockup's
  general disposition — a sidebar of repository-scoped lists, a central
  commit graph, and a details area for changes — orients this layout, and
  that much is a `docs/product/GitSail_PRD_v0.1-v1.0.md`/backlog functional
  requirement (US-053 criterion 1), not a copyright concern. What the
  mockup also shows and this implementation deliberately does **not**
  reproduce: the macOS traffic-light window buttons, the specific icon set
  and multi-page left-hand router-style nav (Overview/Commits/Branches/
  Stashes/PRs/Issues/Files/Diff/Blame/Tags/Remotes/Settings as distinct
  pages), the specific "Recent Activity"/"File Changes"/"Commit Details"
  three-column dashboard composition, or its exact spacing/typography. This
  app has no router — every "page" the mockup implies is a panel that
  already exists as a Vue component, composed here into one sidebar plus a
  tabbed main area instead, which is a structurally different, original
  composition built for the components that actually exist, not a
  recreation of another product's UI.
- **Sidebar scope gap (US-053 criterion 1).** The criterion lists
  "branches/remotes/tags/stashes" for the sidebar. Reading every component
  under `src/components/` before building this layout turned up
  `BranchPanel.vue` (branches), `SyncPanel.vue` (remotes), and
  `PullRequestsPanel.vue` (pull/merge requests) — no tags or stashes
  panel, store, or service exists anywhere in this codebase yet. This
  layout groups what exists (`AppShell.vue`'s sidebar sections); it does
  not invent placeholder tags/stashes UI backed by nothing, since that
  would misrepresent a capability as shipped. This is a real, open gap:
  whichever future story implements tags/stashes should add its panel to
  this same sidebar (`AppShell.vue`'s sidebar `<template v-if="shellState
  .kind === 'ready'">` block is exactly where it belongs).
- **Main area (US-053 criterion 1).** `CommitGraph.vue` is the fixed,
  central `.app-shell__graph` panel. Below it, a tabbed area
  (`role="tablist"`) holds three groups: **Changes** (`StagingPanel` +
  `DiffViewer` — staging and inspecting a diff are the same workflow),
  **Merge & Rebase** (`MergePanel`, which already embeds `RebasePlanPanel`),
  and **Amend** (`AmendPanel`). `StatusPanel` (current branch/clean-dirty/
  refresh/patch export) sits above the graph as the main area's status
  strip, since refreshing and seeing the branch/clean state is relevant
  regardless of which tab is open. Tab panels use `v-show`, not `v-if`, so
  switching tabs never discards in-progress state in another tab (a
  half-typed commit message, a loaded amend preview, an open rebase plan).

## 3. Empty / error / loading states (US-053 DoD)

Resolved once, at the shell level, by the pure `resolveShellState`
(`shellState.ts`, unit tested in `shellState.test.ts`) from
`session.isOpening` / `session.repository` / `session.lastError` — not
duplicated per panel. `AppShell.vue`'s `<main>` renders exactly one of:

| State | When | Presentation |
|---|---|---|
| `opening` | `session.isOpening` | Centered banner, `role="status" aria-live="polite"`, a spinning icon plus the text "Opening repository…" — screen readers are told, not just shown a spinner. |
| `empty` | No repository open, no error | Centered banner with an icon and "No repository open yet." plus a hint pointing at the sidebar's "Browse…"/recent-repository controls, which stay visible in the sidebar the whole time. |
| `error` | Last open attempt failed, still no repository open | `role="alert"` banner with a warning icon, the failure message, and the remediation text when the backend supplied one — never a bare red border with no text (US-055 criterion 3 applies here too). |
| `ready` | A repository is open | The full workspace (status strip, graph, tabs) — regardless of whether a *later* refresh fails; a stale-after-open error is that panel's own concern (e.g. `StatusPanel`'s/`BranchPanel`'s own error handling, already pre-existing), not a reason to blank out a workspace that is otherwise usable. |

This is deliberately narrower than "every panel's own state" — panels that
already had their own explicit empty/error/loading presentation before this
story (e.g. `PullRequestsPanel.vue`'s `noForge`/`noToken`/`rateLimited`/
`offline`/`error`/loaded-but-empty states; `DiffViewer.vue`'s loading/empty/
binary/truncated banners; `BranchPanel.vue`'s `lastError`) keep doing so
unchanged — this story only adds the one missing layer above all of them:
"is there even a repository open to look at yet".

## 4. Accessibility checklist (WCAG 2.1 A/AA, basic)

| # | Check | Status | Where |
|---|---|---|---|
| 1 | Every interactive control has a visible focus indicator | Done | Global `:focus-visible` rule in `theme.css`, applied once to every native control app-wide (no component overrides `outline: none`) |
| 2 | Icon-only or ambiguous (repeated) controls have an accessible name | Done | `aria-label` added to every per-row/per-item button that repeats the same visible text across a list (`BranchPanel`, `StatusPanel`, `PullRequestsPanel`, `MergePanel`, `RebasePlanPanel`, `SearchPalette`); text inputs/textareas with only a placeholder got a paired `<label class="sr-only">` (`SearchPalette`, `RepositoryOpener`, `StagingPanel`, `AmendPanel`, `BranchPanel`, `RebasePlanPanel`) |
| 3 | Every mouse-only action has a keyboard equivalent | Done, one exception documented | Clickable non-button `<span>`s converted to real `<button>`s (`StagingPanel`'s file path, `SearchPalette`'s result rows); `CommitGraph.vue`'s commit list is keyboard-navigable (see §5); its right-click context menu opens via Shift+F10/Menu key. **Exception:** `StagingPanel.vue`'s drag-and-drop stage/unstage already had a button alternative *before* this story (see that file's own comment) — unchanged, still true |
| 4 | Tab/Shift+Tab order is logical and never silently escapes a modal | Done | `ConfirmationDialog.vue`/`HistoryEditingPanel.vue` now trap Tab at their boundaries (`focusTrap.ts` + `keyboardNav.ts`'s `tabTrapIndex`) and move focus in on open |
| 5 | Escape closes/cancels the currently open dialog or menu | Done | Both modal dialogs and the commit-graph context menu |
| 6 | Composite widgets (tabs, listbox, menu) use correct ARIA roles/states | Done | `AppShell.vue` tabs: `role="tablist"/"tab"/"tabpanel"`, `aria-selected`, `aria-controls`; `CommitGraph.vue`: `role="listbox"/"option"`, `aria-selected`, `aria-activedescendant`; context menu: `role="menu"/"menuitem"` |
| 7 | A skip link reaches the main content without stepping through the sidebar | Done | `.skip-link` in `AppShell.vue`/`theme.css` |
| 8 | State changes are conveyed by text/icon, not color alone | Already true app-wide, verified | Every error/warning/success/conflict banner already reviewed in earlier stories pairs its color with explicit text (e.g. "CONFLICT — N file(s)...", "Rate limited...", risk badges' own words); the new shell states (§3) follow the same rule |
| 9 | Toggle-style buttons expose their pressed state to assistive tech, not just a CSS class | Done | `DiffViewer.vue`'s Unified/Side-by-side buttons now also set `aria-pressed` |
| 10 | Color contrast is at least 4.5:1 for normal text in the shipped theme | Verified for new tokens; pre-existing colors not re-audited | See §6 — this is the one item with a documented, scoped-out gap |
| 11 | Layout survives being resized to a narrow window without cutting off actions | Done (CSS-reasoned; no real display in this sandbox — see §6) | `@media (max-width: 60rem)` in `AppShell.vue` stacks the sidebar above the main area instead of clipping it |
| 12 | Only one theme ships in v0.3, but nothing hardcodes colors so a future theme is a token swap, not a rewrite | Done | Every new/touched style reads a `--color-*` custom property from `theme.css`; pre-existing per-component `<style scoped>` blocks with their own hex colors are unchanged (out of scope — see §6) |

## 5. No-mouse flow (US-055 DoD)

Two levels of verification, since this project has no `@vue/test-utils`
and this sandbox has no real display to drive an actual browser/screen
reader with:

**Automated** — `src/components/keyboardNav.test.ts` unit-tests the
underlying navigation math directly, including a test that walks a full
keyboard-only sequence (`ArrowRight` x3, `ArrowLeft`, `Home`, `End`) across
a 3-item tablist and asserts every index was reached — i.e. every tab is
provably reachable by keyboard alone, from the same function `AppShell.vue`
calls. `shellState.test.ts` covers the state each keyboard-only session
would land on depending on whether a repository is open.

**Manually reasoned, end-to-end walkthrough** (traced through the actual
markup/handlers added in this story, in the order a Tab/Arrow-key-only
session would hit them):

1. Page loads. First `Tab` press focuses the skip link (`AppShell.vue`);
   pressing `Enter` (or just continuing to `Tab`) reaches the sidebar's
   "Repository" section — `RepositoryOpener`'s path input, then its
   "Browse…"/"Open" buttons, then `RecentRepositories`' list of buttons —
   with no repository open yet, the main area shows the `empty` state
   (§3) rather than a blank screen. Activating a recent repository (Enter
   on its button) opens it; the shell transitions through `opening` to
   `ready`.
2. Once `ready`, continued `Tab` reaches `SearchPalette`'s labeled search
   input, `BranchPanel`'s per-branch Switch/Rename/Delete buttons (each
   with a disambiguating `aria-label`) and its "new branch name" input,
   `SyncPanel`'s Fetch/Pull/Push buttons, `PullRequestsPanel`'s items.
3. `Tab` reaches the main area's status strip (`StatusPanel`, its
   Refresh button and any per-file patch buttons), then the commit graph
   viewport (a single `tabindex="0"` listbox). `ArrowDown`/`ArrowUp` move
   the selection one commit at a time (non-wrapping — stops at the last
   *loaded* row rather than jumping back to the first); `Home`/`End` jump
   to the first/last loaded row; the viewport auto-scrolls the selection
   into view. `Shift+F10` (or the keyboard `Menu` key) opens the same
   context menu a right-click would, positioned at the selected row, with
   focus moved onto its first item (Copy hash/Cherry-pick/Revert/Reset);
   `Escape` closes it and returns focus to the graph.
4. Choosing "Reset to here…" opens `HistoryEditingPanel.vue`: focus moves
   to its first radio button on open, `Tab` cycles only among its own
   controls (trapped), and `Escape` cancels it and returns focus rather
   than leaking Tab out to the graph behind it.
5. `Tab` continues into the tablist (`role="tablist"`) — only the active
   tab is in the normal tab order; `ArrowLeft`/`ArrowRight` (wrapping) or
   `Home`/`End` move between Changes/Merge & Rebase/Amend, each moving DOM
   focus onto the newly active tab button. `Tab` from the active tab
   button moves into that tab's own panel content next (e.g. Changes:
   `StagingPanel`'s now-keyboard-reachable file buttons, its labeled
   commit-message textarea, its Commit button; DiffViewer's Unified/
   Side-by-side toggle, now exposing `aria-pressed`).
6. Any mutation (stage, commit, switch branch, merge, amend, ...) opens
   `ConfirmationDialog.vue`: same focus-on-open/Tab-trap/Escape-cancel
   behavior as step 4, driven by the same shared `focusTrap.ts`.

No blocking issue was found in this walkthrough. The one intentionally
out-of-scope gap is noted in §6.

## 6. Known gaps / not (fully) covered, and why

- **Color-contrast audit is scoped to the new tokens, not a full repaint.**
  `theme.css`'s own tokens were chosen to clear 4.5:1 against both
  `--color-bg` and `--color-surface` (e.g. `--color-text` #e6edf3, an
  off-white, against #0f1720/#16212c; `--color-danger` #ff6b6b likewise).
  Pre-existing per-component hex colors from earlier stories (e.g.
  `.error { color: #c0392b }` repeated across many panels, `MergePanel`'s
  `.conflict { color: #e0a030 }`) were **not** re-audited or migrated to
  tokens in this pass — doing so would mean editing every panel's
  `<style scoped>` block, which is a repaint, not the layout-consolidation
  this story scopes to. This is the one DoD item left partially open;
  follow-up: migrate those literal colors to the new `--color-danger`/
  `--color-warning` tokens (mostly a value swap, given the tokens already
  land in a similar hue) in a dedicated pass, ideally the same one that
  implements the actual second theme (T-248).
- **No real display/screen reader in this sandbox.** §5's manual
  walkthrough is a code-level trace of the actual markup and handlers, not
  an observed run in a real windowed environment with NVDA/VoiceOver — the
  environment has no display to drive one. The pure navigation math itself
  (§5, "Automated") is unit tested; the DOM wiring around it
  (`CommitGraph.vue`'s keydown handler, `focusTrap.ts`) is exercised by
  `npm run build`'s type-check succeeding and by manual code review, not by
  an executed test, consistent with this project's existing convention of
  not mounting components in tests (no `@vue/test-utils`).
- **Tags/stashes sidebar panels don't exist yet** (§2) — not a US-053/
  US-055 regression, a pre-existing gap in what's been built so far.
- **Responsive behavior is CSS-reasoned, not visually tested** (US-055
  criterion 2) — the `@media (max-width: 60rem)` stacking rule is new and
  untested against a real narrow window; it was designed to fail safe
  (stack instead of clip) rather than verified pixel-by-pixel.
