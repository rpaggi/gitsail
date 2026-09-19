# Preferences matrix — shared vs. interface-specific (v1.0)

Companion to the wiki's "Settings & Preferences Rules" and "Destructive
Operations & Confirmation Guardrails" articles (business rules; consult
those for the *why*, this document for the *where*), to
`crates/gitsail-application/src/preferences.rs` (T-247/US-105), to
`apps/desktop/src/{theme.ts,keybindings.ts}` (T-248/US-106, T-249/US-107),
and to `apps/vscode/src/blameFormat.ts` (T-250/US-108). This is the
design-contract deliverable T-251/US-109 asks for: a single place that
states, for every preference GitSail exposes today, whether it is (a)
genuinely shareable across TUI/Desktop/VS Code, (b) inherently
interface-specific, or (c) conceptually shared but acceptably stored in
separate, interface-owned locations — plus the confirmation-policy
invariant that no preference, in any interface, may ever weaken (criterion
3). Belongs under `docs/architecture/` per `AGENTS.md`'s own rule ("the
wiki stores business rules only; architecture/product documents belong
under `docs/`"), not the wiki.

## How to read the matrix

- **Shared** — the same concept, and ideally the same value, should apply
  regardless of which interface a person has open. A theme is the person's
  preference, not the interface's.
- **Interface-specific** — the concept itself does not transfer. VS Code's
  native keybindings system is not something GitSail should shadow with its
  own; the TUI has no analogue of a Desktop global hotkey chord.
- **Shared, separately stored (acceptable)** — the concept is shared, but
  each interface currently persists its own copy rather than reading one
  common store. This is explicitly allowed by the wiki's own precedence
  rule ("repository-level override only where explicitly justified" — the
  same "only add the tier you actually need" spirit applies here to
  cross-interface sharing) as long as it is a deliberate, documented
  decision rather than an accident of three teams never comparing notes.

| Preference | Category | Current storage per interface | Decision / rationale |
|---|---|---|---|
| Theme (dark / light / system) | Shared, separately stored (acceptable) | Desktop: `ThemePreference` in `gitsail_application::preferences`, persisted by `apps/desktop/src-tauri/src/preferences_store.rs` (`<config dir>/gitsail/desktop/preferences.json`). VS Code: none — relies entirely on the editor's own `workbench.colorTheme` (GitSail contributes no color/theme setting of its own). TUI: no light/dark concept at all — `--ascii`/`NO_COLOR` is a *rendering fidelity* switch (color vs. text-only), not a theme choice; the terminal emulator, not GitSail, owns light/dark for a TUI. | A real, shared *concept* ("does this person prefer a dark or light surface") that today has only one real implementation (Desktop). VS Code correctly has none — an editor extension re-implementing theming alongside the host editor's own would be exactly the "advanced features degrade the basic experience" (Product Principles rule 7) and "GUI/TUI are their own experiences" (rule 5) anti-pattern; VS Code's theme is the editor's theme, full stop. The TUI genuinely has no equivalent slider — it never renders its own chrome color independent of the terminal's palette. **No migration needed**: there is nothing to consolidate because there is only one store today. If VS Code or the TUI ever grow their own themable surface, that store should reuse `gitsail_application::ThemePreference`'s three-value shape (`System`/`Light`/`Dark`) rather than inventing a fourth vocabulary. |
| Date rendering (absolute `YYYY-MM-DD` vs. relative "3 days ago") | Shared concept; one alignment fix applied, one gap left open | VS Code: `BlameDateStyle` (`"relative"` \| `"absolute"`) in `apps/vscode/src/blameFormat.ts`, read from `gitsail.blame.dateStyle` (T-250/US-108), defaulting to `"absolute"`. TUI: `crates/gitsail-tui/src/ui.rs::format_timestamp` — **fixed by this task** (see below) to render an absolute calendar date/time in the commit's own recorded offset, matching VS Code's `formatGitTimestamp`'s exact algorithm (shift by offset, read UTC fields off the result — never the host machine's local timezone), instead of a raw Unix epoch integer. No relative-date choice exists in the TUI yet. Desktop: no commit date is rendered anywhere in the UI today (`GitTimestampDto` exists in `services/dto.ts` but nothing currently formats it for display) — nothing to align yet. | This is the concrete "align formats where cheap" case criterion 1 asks for. Before this task, the TUI was the one place in the workspace still showing a raw epoch integer (`1705314600 (+02:00)`) instead of a calendar date — genuinely inconsistent with both VS Code's blame decorations and with what any person would expect. Fixing the *absolute* rendering to agree was cheap (no new dependency; reused the same integer civil-calendar algorithm `blameFormat.ts`'s own doc comment already describes) and is done. Introducing a TUI-side *relative* date style, and a single shared `DatePreference` type in `gitsail_application` both VS Code and the TUI read from, is a reasonable follow-up but is explicitly **not** done here — it would need its own use case, storage decision and settings surface in each interface, which is more than "cheap." Tracked as a deliberate gap, not an oversight. |
| Diff rendering (unified vs. side-by-side) | Interface-specific | Desktop: `apps/desktop/src/components/diffPresentation.ts` derives both `unifiedLines` and a side-by-side layout from the same `DiffHunkDto` (US-057) — a person picks a view, presumably per Desktop session/preference (not yet persisted as a named preference itself). TUI: `crates/gitsail-tui/src/ui.rs`'s Diff panel only ever renders a unified view — a terminal's fixed-width, single-pane layout has no natural side-by-side equivalent the way a resizable GUI panel does. VS Code: renders through the editor's own native diff/decoration surfaces, not a GitSail-owned layout at all. | Not a shareable preference: "unified vs. side-by-side" is a choice that only makes sense where both a GUI panel width and a person's screen make a second column viable. The TUI's terminal width usually cannot support it at all (see US-043's minimum-size handling), and VS Code deliberately never reimplements diff rendering the editor itself already owns. No consolidation attempted. |
| Commit graph layout (lane/connector style) | Interface-specific, aligned by contract not by preference | Both Desktop (`commitGraphLayout.ts`) and the TUI (`gitsail-domain`'s shared `graph` module, consumed via `crates/gitsail-tui/src/graph_view.rs`) already derive their lane assignment from the **same** underlying `CommitGraph`/lane-stability contract (`gitsail_domain::graph`'s own module doc is the single source of truth both presentations render from) — see US-064/US-065. VS Code has no commit-graph view. | Already correctly shared at the *data* layer (one lane-assignment algorithm, not two independently invented ones) — this is the model for how a genuinely cross-cutting concept should be aligned: not a user-facing "preference" at all, just one Core algorithm two renderers draw from. Nothing further to do here; listed for completeness since T-251 explicitly calls out "graph" formats. |
| Keyboard shortcuts / keybindings | Interface-specific by construction | Desktop: `apps/desktop/src/keybindings.ts` — a small allow-list of global `Mod+...` chord shortcuts (T-249/US-107), remappable via `KeybindingsPanel.vue`, persisted through `stores/keybindings.ts`. TUI: **new in this task** — `crates/gitsail-tui/src/keybindings.rs`, a small allow-list of single-character shortcuts (T-251/US-109 criterion 2; see below), remappable via a plain-text config file. VS Code: no GitSail-owned keybinding surface at all — every GitSail command is contributed to VS Code's own `Contributions.commands`, and rebinding it is done through the editor's own native `keybindings.json`, exactly like any other extension's commands. | Genuinely not shareable, and deliberately not unified into one format: a Desktop global hotkey (`Mod+Shift+F`, chord-based, competing with the OS/browser's own reserved combinations) and a TUI single-character shortcut (modal, no chord, competing with nothing but plain typing while a text field has focus — see `keybindings.rs`'s own "scope decision" doc) are different *interaction models*, not the same concept serialized two ways. Forcing them into one config format would either cripple the TUI's simplicity or make Desktop's global shortcuts unusably terse. VS Code correctly has no GitSail-specific mechanism here at all: `Contributions.commands` is the paved path every VS Code extension uses, and building a parallel one would be exactly the "advanced features degrade the basic experience" anti-pattern. **Two independent registries is the correct, final design here — not an interim state**, but both follow the identical *shape* (a fixed allow-list of the most common commands, each with one documented default, explicitly excluding anything that would let a remap bypass a confirmation) — see the confirmation-policy section below for why that shape is what actually matters, not the storage format. |
| Blame display (format template, current-line vs. all-visible-lines, delay) | Interface-specific feature, shareable format-syntax where it exists | VS Code only: `apps/vscode/src/blameFormat.ts`'s `BlameDisplayConfig` (T-205/US-072, T-250/US-108) — a `${author}`/`${date}`/`${hash}`/`${shortHash}`/`${message}` template. The TUI has its own, separate blame view (`b` toggles `DiffViewMode::Blame`) with a fixed, non-configurable rendering — no template, no date-style choice. Desktop has no dedicated blame view. | VS Code's inline-decoration blame is a feature shape (per-line ambient annotation next to code) that has no TUI/Desktop equivalent to align *with* today — the TUI's blame view is a dedicated full-panel view, a different UI shape entirely, not a rendering choice on the same feature. Nothing to consolidate now. If the TUI's blame view ever grows a configurable format, it should reuse VS Code's placeholder vocabulary (`${author}`, `${date}`, ...) rather than invent a second one — noted here so that decision is made consciously later, not accidentally differently. |
| Precedence model (defaults → user → repository) | Shared rule, not shared storage | `gitsail_application::preferences`'s own module doc states the rule explicitly for Desktop's `Preferences`/`ThemePreference` (defaults → user; no repository tier exists yet, deliberately). The TUI's new keybindings-override file (this task) is a single, always-user-level tier — no defaults-vs-user distinction beyond "file present or not," and no repository tier. | The *rule* (from the wiki: "defaults → user → repository, repository only where justified") is shared and must be honored by any preference any interface adds — it is not itself a piece of state to store anywhere. Every preference in this matrix currently only uses the first two tiers; none has needed a repository-level override yet, so none has speculatively built one (matches `preferences.rs`'s own stated reasoning for not pre-building unused scope). |

## TUI keyboard remapping (criterion 2)

Before this task, `crates/gitsail-tui/src/keymap.rs` was entirely
hardcoded: one `match` from a raw `KeyCode` to an `Action`, per
`InputContext`, with no override mechanism at all. T-251 adds one, without
building a Vim/Emacs mode (explicitly out of scope):

- **`crates/gitsail-tui/src/keybindings.rs`** defines `CONFIGURABLE_ACTIONS`
  — a fixed, documented allow-list of ten common, already-existing
  `Normal`-context shortcuts (`quit`, `toggle-help`, `refresh`,
  `toggle-stage`, `start-commit`, `request-fetch`, `request-pull`,
  `request-push`, `toggle-blame-view`, `start-search`), each with its
  existing default single-character key. This mirrors
  `apps/desktop/src/keybindings.ts::CONFIGURABLE_ACTIONS`'s own shape (a
  small registry of id/label/default, resolved through an `overrides` map),
  translated to the TUI's own single-character convention instead of
  Desktop's `Mod+...` chords.
- A plain-text config file (default location
  `<OS config dir>/gitsail/tui/keybindings.conf`, overridable with
  `gitsail-tui --keybindings <path>`), one `action-id = X` assignment per
  line, `#` for comments. `crate::keybindings::parse_config` never fails
  outrightly — an unknown id or a value that is not exactly one character
  is reported as a warning (printed to stderr at startup) and ignored, the
  file falls back to defaults, exactly like an invalid `preferences.json`
  falls back to `Preferences::default()` (US-105 criterion 3's same
  discipline, applied here).
- `crate::keymap::resolve_action` is the new entry point `main.rs`'s event
  loop calls instead of `action_for` directly: it consults the resolved
  bindings **only** while `InputContext::Normal` is active; every overlay,
  prompt and dialog context still resolves exclusively through the
  original, hardcoded `action_for`.

No Vim/Emacs modal editing, no per-motion remapping, no macro system — a
person can change which single key opens the commit composer or triggers a
fetch, nothing more, exactly matching criterion 2's own scope cut.

## The confirmation-policy invariant (criterion 3) — audit and guarantee

The wiki's "Settings & Preferences Rules" article states this exactly:
*"Confirmation and guardrail policies for destructive operations can never
be disabled through settings."* This section is the audit T-251 asks for,
plus where each interface's structural guarantee actually lives.

**Core (`gitsail-application::mutation::RiskLevel`)**: `skips_confirmation`
returns `true` only for `RiskLevel::Safe`, and `requires_reinforced_confirmation`
returns `true` only for `RiskLevel::Destructive` — both are `const fn`s
over a fixed three-variant enum, not parameterized by any preference,
config, or environment value anywhere in the codebase. There is no code
path through which a preference could change what these functions return.

**TUI (`crates/gitsail-tui::operation::OperationState`)**: a `Moderate`/
`Destructive` mutation always starts in `Confirming`, and the *only* way to
reach `InProgress` is `OperationState::confirm`, called from
`App::handle_activate` — itself only ever invoked in response to
`Action::Activate`. The new keybindings-override mechanism cannot affect
this for two independent, structural reasons (either one alone would be
sufficient):

1. `Action::Activate` and `Action::Dismiss` are not, and can never become,
   entries in `CONFIGURABLE_ACTIONS` — see `keybindings.rs`'s own module
   doc for why an override file cannot name them (there is no id to
   resolve to). Enforced by
   `keybindings::tests::the_registry_never_lists_activate_or_dismiss` and
   `keybindings::tests::attempting_to_remap_activate_or_dismiss_is_rejected`.
2. `keymap::resolve_action` only ever consults the bindings table while
   `InputContext::Normal` is active — every overlay/confirmation context
   (`ResetMode`, `Amend`, `RebasePlan`, ...) resolves through the original
   `action_for` alone, regardless of what the bindings table contains, even
   an adversarially-constructed one. Enforced by
   `keymap::tests::bindings_are_never_consulted_outside_the_normal_context`.

The end-to-end proof lives in
`crates/gitsail-tui/src/app.rs`'s test module:
`a_destructive_reset_still_requires_two_explicit_confirmations` drives a
real `Hard` reset (SAD §20's own named `Destructive` example) through the
full `App::update` state machine and asserts that exactly two distinct
`Action::Activate` dispatches are required before a `Command::Reset` is
ever produced — the first only opens the mode chooser or starts
confirmation, dispatching nothing; only the second actually mutates.
`dismissing_a_pending_destructive_reset_confirmation_dispatches_nothing`
proves the companion guarantee (wiki rule 5: cancelling before confirmation
leaves the repository untouched).

**Desktop (`apps/desktop/src/stores/operation.ts`)**: `request()` skips the
explicit confirmation step only when `op.risk === "safe"` — a string
literal comparison against a value that is itself always a hardcoded
literal at every call site (`stores/sync.ts`, `stores/branches.ts`,
`stores/reset.ts`, `stores/amend.ts`, `stores/merge.ts`, ...), never read
from `stores/keybindings.ts`, `preferences.ts`, or any other
user-configurable source. The T-249/US-107 keybindings remap this task was
asked to audit only ever changes which key triggers one of five fixed
`AppShell.vue::GLOBAL_ACTION_HANDLERS` entries (`focus-search`, `fetch`,
`pull`, `push`, `commit`) — a closed map to five already-existing,
already-Safe/Moderate-risk store calls; none of the five is, or can become
without an explicit code change to that map, capable of reaching a
`"destructive"`-risk operation. This audit is made durable by
`keybindings.test.ts`'s new canary test
(`T-251/US-109: configurable-actions allow-list stays a closed, audited
set`), which fails the moment a new configurable action id is added
without a matching, conscious update to that exact list — forcing whoever
adds it to re-confirm it still only reaches a non-destructive call.
**No fix was needed** — the audit found the existing T-249 implementation
already sound — but the guarantee is now pinned by a test rather than
resting on inspection alone.

## Architecture keeps a door open to future translation (criterion 3, i18n)

No i18n is implemented in this change — none was asked for. What matters
is not foreclosing it later:

- Every new user-facing string this task adds (the TUI's config-parse
  warnings, the `ConfigurableAction::label`s) is a complete, whole
  sentence or noun phrase built with named interpolation
  (`format!("line {line_no}: ...")`), never assembled by concatenating two
  independently-"translatable" fragments end to end (e.g. never
  `"line ".to_string() + &line_no.to_string() + ": expected..."` split
  across two separately-localizable pieces) — concatenation like that is
  exactly what breaks a future translation, since word order and
  punctuation differ per language. `OperationKind::prompt_label` (in
  `crates/gitsail-tui/src/operation.rs`, predating this task as
  `target_label` and renamed by T-267) already follows the same discipline
  and was left unchanged as the existing, correct precedent.
- `ConfigurableAction::label` (`"Quit"`, `"Toggle help"`, ...) is a single,
  free-standing field per action — a future localization pass swaps its
  value per-locale without touching `id` (the stable, never-localized
  config-file key) or `action` (the behavior). Keeping the stable
  identifier and the human label as two separate fields, rather than
  deriving one from the other, is what a translation layer needs.
- No new string in this change is split across multiple UI elements in a
  way that assumes a fixed word order (e.g. "reset mode: " + modeName +
  " (destructive)" laid out as three separately-styled widgets that must
  always read left-to-right) — every new label is one complete string
  handed to one widget.

## Definition of Done cross-check

- **DOD-G**: this change adds tests (`cargo test --workspace`), passes
  `cargo clippy --workspace --all-targets -- -D warnings` with zero
  warnings, and (for the touched `apps/desktop` file) passes
  `npm run test -- --run` and `npm run build`. English-only new artifacts.
  No credentials/internal hosts introduced.
- **Matrix of scopes and migration reviewed**: this document. No migration
  is required — every "separately stored" preference in the table above is
  judged acceptable as documented, and the one concrete alignment
  opportunity found cheap enough to act on (the TUI's raw-epoch date
  format) was fixed in the same change; the one identified as non-cheap
  (a shared relative/absolute `DatePreference` type) is named as a
  deliberate, tracked gap rather than silently left unstated.
- **TUI defaults and shortcut tests approved**: `crates/gitsail-tui/src/
  keybindings.rs` and `crates/gitsail-tui/src/keymap.rs` cover config
  parsing, default resolution, override resolution, conflict detection, and
  — the load-bearing case — that the registry and the resolution boundary
  both make an `Activate`/`Dismiss` override structurally impossible;
  `crates/gitsail-tui/src/app.rs` covers the full destructive-confirmation
  flow end to end.
