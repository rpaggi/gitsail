# Test coverage audit — T-255/US-122

This document is the record of the audit T-255/US-122 ("Cobrir fluxos
críticos das interfaces") required before closing any gap: what the v0.4
test suites (TUI, Desktop, VS Code extension) already covered, what was
genuinely missing against the story's three acceptance criteria, and what
was added to close only the real gaps. Unlike the other EPIC-24 stories,
this one is primarily an audit — most of the suite already existed and did
not need re-auditing from scratch (DoD-G: "cada release anterior já aplica
sua parte pela DoD global").


> **Update (ADR-025, 2026-09-19):** this is a historical audit record and is
> deliberately left as written. For the VS Code extension it is now partly
> out of date: ADR-025 moved the extension off the `gitsail` CLI onto `git`
> directly, deleting `src/cliClient.ts`, `src/cliLocator.ts`,
> `src/cliResult.ts` and `src/protocol.ts` together with
> `test/cliClient.test.ts`, `test/cliLocator.test.ts`,
> `test/cliResult.test.ts`, `test/protocol.test.ts`,
> `test/protocolCompatibility.test.ts`, `test/coreParity.test.ts` and the
> `test/fixtures/fake-cli.js` stub. Every citation of those files below
> should be read as describing the suite as it stood at the time of the
> audit. The coverage itself was not dropped: the data-access suites were
> rewritten against real temporary repositories (`test/gitProcess.test.ts`,
> `test/gitClient.test.ts`, plus the rewritten service suites), and
> `test/gitParity.test.ts` added a check the suite did not previously have
> — the extension's own DTOs compared against the real `gitsail` CLI's. The
> audit's *findings* about which flows needed covering are unaffected; only
> the file names and the layer they sit at changed.

## Method

Three independent read-only surveys (one per interface) inventoried every
existing test file against the story's four scenario categories (failure,
cancellation, empty repository, malicious content) and its two structural
criteria (TUI snapshot tests; Desktop/VS Code end-to-end chains), each
citing concrete `file:line` evidence. Gaps were closed only where the
survey found a genuine absence — no test was added merely to raise a count.

## Criterion 1 — TUI: state/update tests since v0.2, and snapshot tests

**Already true:** every integration test in `crates/gitsail-tui/tests/`
dispatches `Action`s through `App::update`, drives the resulting
`Command`s against a real, temporary Git repository via `GitCliProvider`
(never a mock), and asserts on the resulting model state — this has been
true since the TUI's foundation (EPIC-09) and was not re-audited file by
file.

**Gap found:** a render-to-text helper (`tests/support::buffer_text`) is
used widely, but every existing assertion on rendered text is a substring
`text.contains(...)` probe — there was no test comparing a full rendered
frame (or a specific region of one) against a fixed, exact expected
string. `tests/merge_conflicts.rs` in particular had **zero** rendering
assertions at all (state only), despite covering the highest-risk screen
in the product (T-231/T-232/T-233).

**Closed:** `crates/gitsail-tui/tests/snapshot_screens.rs` (new file), three
tests, each rendering against `ratatui::backend::TestBackend` and
comparing the captured text with a plain `assert_eq!` against a
hand-written expected string (no external snapshot-testing crate, per the
story's own suggestion):

- `a_single_commit_graph_panel_frame_matches_the_recorded_snapshot` — the
  Graph panel example the story names explicitly.
- `the_merge_conflicts_overlay_popup_matches_the_recorded_snapshot` — the
  Conflicts panel example the story names explicitly; this is the first
  rendering assertion `tests/merge_conflicts.rs`'s own scenario ever gets
  (asserted from a sibling file, since `merge_conflicts.rs`'s own drive-loop
  helpers had to be duplicated to reach the same state before rendering —
  see "Design decisions" below).
- `an_empty_repository_frame_matches_the_recorded_snapshot` — doubles as
  part of the "empty repository" scenario coverage below.

Two determinism hazards had to be handled explicitly (both documented in
the file's own module doc comment), since these run against a real,
temporary Git repository on every one of `ci-policy.md`'s three OS legs:

1. **Commit hashes** are content-derived (author/committer timestamps are
   part of what Git hashes), so they differ on every run. The graph-panel
   snapshot queries the real short hash right after creating the commit and
   substitutes a fixed placeholder for it before comparing — the expected
   string itself never embeds a hash.
2. **The Sidebar's `repo: <path>` line** embeds the fixture's absolute
   temporary-directory path, which varies in both content and length
   across runs and operating systems (and gets truncated to the column's
   fixed inner width by Ratatui, so the *visible* substring is never even
   the same length as the real path). A small `mask_repo_path_line` helper
   replaces whatever sits between `"repo: "` and the panel's closing border
   with a fixed run of `·` of the same width, applied identically to both
   the actual and the expected string.

All three tests were run to completion, their exact rendered output
captured, and that captured output (after masking) used as the recorded
expected string — never guessed by hand — then re-run several times in a
row locally to confirm determinism before being committed to the suite.

Two smaller, non-snapshot gaps this same audit found under "TUI" were
closed alongside the snapshots (see "Criterion 3" below):
`commit_graph.rs::a_malicious_commit_subject_is_sanitized_before_rendering_in_the_graph_panel`
and
`merge_conflicts.rs::declining_a_pending_merge_confirmation_dispatches_nothing_and_leaves_head_untouched`.

## Criterion 2 — Desktop E2E since v0.3; extension lifecycle/blame/history in v0.4

### Desktop

**Already true:** every critical flow has thorough *isolated* coverage —
staging, committing, merging, resolving a conflict, and continuing an
operation each have their own well-tested store/service, and failure paths
for each are covered too.

**Gap found:** no single test walked "open → status → stage → commit" or
"detect conflict → resolve → continue" end to end in one continuous
sequence; each step existed as its own separate test starting from a
freshly reset store, never chained from the previous step's real
resulting state.

**Closed**, both added to their existing store test files (matching the
project's established Pinia store + mocked-Tauri-IPC pattern — there is no
`@vue/test-utils` E2E harness in this project, by design, since Tauri
components cannot be driven from a plain Vitest/jsdom process):

- `stores/staging.test.ts` — a new test chains `session.openRepository()`
  (with real unstaged/untracked files already present) → `stageFiles()` →
  `requestCommit()` → `operation.confirm()`, asserting the fully-staged
  intermediate state and the final clean/committed state in one test.
- `stores/merge.test.ts` — a new test chains `requestMerge()` (conflict
  outcome) → `markResolved()` → `requestContinue()` → `operation.confirm()`,
  asserting the repository is genuinely conflict-free and the pending
  operation is cleared only after the explicit continue.

### VS Code extension

**Already true:** blame, commit details, file history, and line history
each have solid isolated coverage in `historyController.test.ts`, each in
its own `describe` block with a fresh `HistoryController` built from
scratch.

**Gap found:** no test ever built `ExtensionController` and
`HistoryController` together and wired
`controller.onRepositoryContextChanged` into
`historyController.onRepositoryContextChanged` the way `extension.ts`
actually does — every existing test called
`controller.onRepositoryContextChanged(client, REPO_ROOT)` directly as
setup, bypassing the real activation sequence entirely. No test walked
activation → blame → commit details → file history in one chain.

**Closed:** `test/extensionLifecycleFlow.test.ts` (new file) — one test
that reproduces `extension.ts::activate()`'s exact sequence (create
`ExtensionController`, create `HistoryController`, wire the repository
context hook, only then activate) against a single fake CLI client
dispatching on `args[0]`, then drives, in order: activation (repository
resolved, status bar shows the branch) → a blame recompute (the active
line gets a decoration whose hover references the real commit) → opening
that commit's full details (`gitsail-commit:` document, re-queried, never
the decoration string reused) → browsing that file's history (the same
commit appears in the picker). `FakeHost`/`FakeHistoryHost` are trimmed,
self-contained copies of the doubles already established in
`controller.test.ts`/`historyController.test.ts` (see "Design decisions").

## Criterion 3 — failure, cancellation, empty repository, malicious content

| Scenario | TUI | Desktop | VS Code extension |
| --- | --- | --- | --- |
| Failure | Already covered (open/commit/rebase/push failures, `tests/smoke.rs`, `commit_composer.rs`, `rebase.rs`, `remote_sync.rs`) | Already covered (open/commit/sync/rebase failures across `stores/*.test.ts`) | **Gap closed**: `showFileHistory` against a CLI domain-error now asserted (`historyController.test.ts`) |
| Cancellation | **Gap closed**: declining a *merge* confirmation (`merge_conflicts.rs`) — the existing pattern only covered `Reset` | Already covered (`operation.test.ts`'s decline-before-confirm — the only cancellation semantics this product has; there is no mid-flight abort of a running Git process, by design) | **Gap closed**: dismissing (Escape) a *non-empty* file-history quick pick (`historyController.test.ts`) — only the empty-list case existed before |
| Empty repository | Already reasonably covered (`smoke.rs`, `references_panel.rs`); **snapshot added** (`snapshot_screens.rs`) | **Gap closed**: opening an unborn-HEAD repository end to end (`stores/session.test.ts`); an empty `get_commit_graph_page` response (`stores/graph.test.ts`) | **Gap closed**: blaming a file against an unborn-HEAD domain-error response (`historyController.test.ts`) |
| Malicious content | Only file names were covered before; **gap closed**: a commit subject with ANSI/control bytes reaching the Graph panel (`commit_graph.rs`) | Only PR titles/patch paths were covered before; **gap closed**: control characters/ANSI/an extremely long string in a searched commit subject and branch name (`stores/search.test.ts`) | `hoverSanitizer.test.ts` already covered hover Markdown thoroughly; **gap closed**: a hostile commit subject (control chars, an embedded CR, codicon-like `$(...)` syntax, an extremely long run) reaching a QuickPick label, plus a multi-line subject attempting to spoof extra rows (`historyPresentation.test.ts`) |

Deliberately **not** added, and why:

- **Desktop mid-flight cancellation of sync/push/fetch.** No such feature
  exists in `apps/desktop/src` (confirmed by a repo-wide search) — GitSail's
  only cancellation semantics anywhere in this product is "decline before
  confirmation" (Destructive Operations & Confirmation Guardrails wiki rule
  5), already covered. Inventing a test for an abort feature that does not
  exist would test nothing real.
- **VS Code `AbortSignal` propagation into blame/history CLI calls.** The
  controller genuinely never plumbs a cancellation token into these calls
  today (confirmed by reading `historyController.ts` in full) — this is a
  potential product gap, not a test gap, and changing that behavior is out
  of scope for a testing story. The QuickPick-dismissal tests above are the
  actual, already-supported cancellation surface for these flows.
- **Line-history quick-pick dismissal**, duplicating the same file-history
  dismissal test against an identical code path
  (`if (!selected || selected.id === NO_HISTORY_ITEM_ID) return;` is shared
  logic) — would be redundant coverage of the same branch, not a new gap.
- **A dedicated fake-cli.js empty-repo fixture mode.** Not needed: the
  in-memory `makeClient`/`FakeHistoryHost` doubles `historyController.test.ts`
  already uses (not the spawned-process `fake-cli.js`, which only backs
  `cliClient.test.ts`/`cliLocator.test.ts`) are the right layer for this
  gap and needed no new fixture machinery.

## Design decisions worth flagging

- **`snapshot_screens.rs` duplicates `merge_conflicts.rs`'s fixture/drive-
  loop helpers** (`open_and_load`, `run_mutation`, `select_branch`,
  `setup_conflicting_divergence`) rather than importing them. Rust
  integration test binaries cannot share code except through `mod support`
  (already shared); every existing file in this directory already accepts
  this kind of small duplication (`tests/support/mod.rs`'s own doc comment:
  "Not every test binary in `tests/` uses every helper here"). Extracting a
  second shared module for just these four functions was judged not worth
  the churn against the existing, working `merge_conflicts.rs`.
- **The merge-conflicts overlay snapshot needed an extra `Action::Dismiss`**
  the equivalent *state* test in `merge_conflicts.rs` never needed: after a
  conflicting merge, `ui.rs::render_overlays` shows the merge's own
  "Done — press any key to dismiss" result overlay ahead of the conflicts
  overlay, purely a rendering-priority concern the state-only test never
  observes. This was discovered empirically (the popup capture showed the
  wrong overlay) and is now called out in the test's own comment so it is
  not "mysteriously" required next time this file is touched.
- **Extending `extensionLifecycleFlow.test.ts` with trimmed copies of
  `FakeHost`/`FakeHistoryHost`**, rather than importing the full versions
  from `controller.test.ts`/`historyController.test.ts` (neither file
  exports its doubles) or extracting a shared fixture module. A shared
  module was considered and rejected for this story: it would touch two
  already-passing, already-reviewed test files for a refactor unrelated to
  what this story asks, for a one-file benefit.
- **VS Code's "empty repository" and "failure" tests reuse the same
  `CliResult` domain-error/cli-unavailable shapes** `cliResult.test.ts`/
  `cliClient.test.ts` already establish, rather than inventing a new
  failure taxonomy — an unborn-HEAD blame failure and a generic CLI failure
  are both, correctly, just a `kind !== "ok"` result to `historyController.ts`,
  and the tests assert that identical handling rather than pretending the
  code path is more special-cased than it is.

## Verification

Run from a clean checkout, repeated several times to confirm determinism
(no flakiness observed in any repetition):

- `cargo test --workspace` — all green (workspace total, TUI crate alone:
  smoke 4, status_and_diff 6, commit_graph 5, merge_conflicts 5,
  snapshot_screens 3 — new — plus every other existing suite unchanged).
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `cargo fmt --all -- --check` — clean.
- `apps/desktop`: `npm run test -- --run` — 41 files, 300 tests, all green;
  `npm run build` (`vue-tsc --noEmit && vite build`) — clean.
- `apps/vscode`: `npx vitest run` — 23 files, 202 tests, all green;
  `npx tsc -p ./` — clean.

## Files changed

- `crates/gitsail-tui/tests/snapshot_screens.rs` (new) — 3 snapshot tests.
- `crates/gitsail-tui/tests/commit_graph.rs` — 1 new malicious-content test.
- `crates/gitsail-tui/tests/merge_conflicts.rs` — 1 new cancellation test.
- `apps/desktop/src/stores/staging.test.ts` — 1 new E2E chain test.
- `apps/desktop/src/stores/merge.test.ts` — 1 new E2E chain test.
- `apps/desktop/src/stores/session.test.ts` — 1 new empty-repository test.
- `apps/desktop/src/stores/graph.test.ts` — 1 new empty-repository test.
- `apps/desktop/src/stores/search.test.ts` — 1 new malicious-content test.
- `apps/vscode/test/extensionLifecycleFlow.test.ts` (new) — 1 E2E lifecycle
  chain test.
- `apps/vscode/test/historyController.test.ts` — 3 new tests (failure,
  cancellation, empty repository).
- `apps/vscode/test/historyPresentation.test.ts` — 2 new malicious-content
  tests.
- `docs/architecture/test-coverage-audit-us122.md` (this file).
