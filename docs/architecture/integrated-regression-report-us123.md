# Integrated platform regression report — T-256/US-123

This is the closing report for EPIC-24 ("Testing & Quality"): T-256/US-123,
"Validar plataforma integrada e regressões avançadas." Where T-255/US-122
audited each interface's *own* critical-flow coverage in isolation, this
story asks a cross-cutting question: given that TUI, Desktop, and VS Code
all sit on the same `gitsail-application`/`gitsail-git` Core, is that
actually proven, is the advanced-operation/interruption/installation matrix
documented per interface, and does any code path secretly depend on a
forge account or network access for a local Git flow? Each of the three
acceptance criteria below is answered with concrete `file:line` evidence,
not narrative.

## Criterion 1 — the same scenario produces an equivalent Git state in TUI/Desktop

**Method.** Read-only inspection first: `crates/gitsail-tui/src/worker.rs::spawn_one`
and `apps/desktop/src-tauri/src/commands.rs`'s command handlers were
compared call site by call site. Every mutation/read in both is exactly one
`gitsail_application::<UseCase>::new(port).execute(...)` call, given the
same repository handle and arguments — Desktop additionally wraps each call
in `run_mutation` (an epoch/staleness guard specific to Tauri's own
multi-window `AppState`, `commands.rs:64-79`), which has no bearing on the
resulting Git state. This is what makes "TUI and Desktop produce the same
Git state" true by construction rather than by luck.

**Proof, not assumption.** `crates/gitsail-application/tests/tui_desktop_parity.rs`
(new) replays, against two independently created (never shared) temporary
repositories seeded identically, the exact use-case call sequence each
interface's own call site issues, for two representative scenarios:

- `create_branch_two_commits_conflicting_merge_resolve_continue` — create a
  branch, commit twice on it, diverge `main`, merge (conflict), resolve,
  continue (T-231/T-232/T-233's own flow, the highest-risk sequence in the
  product).
- `amend_last_commit` — the preview/amend flow (T-242/US-090), the other
  scenario this story names explicitly.

Each scenario has a `replay_as_tui` and a separate `replay_as_desktop`
function (deliberately not merged into one shared helper — merging them
would hide exactly the divergence this test exists to rule out), each citing
the real source location it mirrors. Both are then asserted, via
`final_state` (which reads back through the same `ListBranches`/`GetCommit`
read use cases the Graph/Branches panels themselves use, never raw `git log`
text parsing), to produce:

- the same set of branches, each with the same `is_current` flag;
- the same tip **tree hash** per branch (content-only, timestamp-independent
  — see the file's module doc for why a raw commit hash is the wrong
  equality check here: two real, wall-clock `GitCliProvider` mutations run
  at different real times legitimately produce different commit object
  hashes for identical content, the same hazard T-255's own
  `snapshot_screens.rs` hit and solved by masking rather than comparing);
- the same commit subject and parent count per branch tip (main's tip is
  asserted to be a genuine two-parent merge commit in the conflict
  scenario, and the amend scenario's tip is asserted to still have zero
  parents).

Both tests pass (`cargo test -p gitsail-application --test
tui_desktop_parity`: 2 passed). `crates/gitsail-application/Cargo.toml`
gained one dev-dependency, `gitsail-test-support` (a dev-dependency cycle
already an established pattern — see the same comment already on
`crates/gitsail-git/Cargo.toml`).

## Criterion 2 — VS Code presents hashes, authorship, and diffs coherent with Core

**Method.** Every *existing* VS Code presentation test
(`commitDetailsText.test.ts`, `historyPresentation.test.ts`,
`blameFormat.test.ts`, …) exercises the extension's formatting functions
against a hand-written fixture DTO. A fixture can never catch a divergence
between what the extension *assumes* a DTO field means and what
`gitsail-cli` actually puts there for a real commit.

**Proof, not assumption.** `apps/vscode/test/coreParity.test.ts` (new)
builds the real `gitsail` binary (`cargo build --quiet -p gitsail-cli`),
creates a real, temporary two-commit Git repository, and runs the exact
same `commitService.ts`/`blameService.ts`/`fileHistoryService.ts` wrapper
functions `historyController.ts` itself calls in production — never
hand-built JSON — then feeds the resulting real DTOs into the extension's
own presentation functions. Four tests, all passing:

1. `renderCommitDetailsText` — asserts the exact 40-character hash appears
   on its own `commit <hash>` line (never Core's own `shortHash`, checked
   as an *exact line* match rather than a substring — a substring check
   can never fail here, since `shortHash` is a genuine prefix of `hash`)
   and that `Author:    {name} <{email}>` matches Core's `CommitDto.author`
   verbatim, in the same field order `gitsail-cli`'s own
   `output.rs::render_commit_block` uses.
2. Blame hover/decoration (`describeBlameLine`) — cross-checks a real
   blamed line's hash/author against a **second, independent** `gitsail
   commit <hash>` call for that same hash, then asserts the hover embeds
   the full hash verbatim and the inline `${shortHash}` is a genuine prefix
   of it (the "hash truncado errado" hazard this story names explicitly).
3. Commit-diff file picker (`buildCommitDiffFileQuickPickItems`) and
   `describeCommitDiffBase` — cross-checks `commit-diff`'s reported `base`
   against `commit HEAD`'s own `parents[0]` (two separate Core calls
   agreeing), and asserts the changed file list matches.
4. File-history quick pick (`buildFileHistoryQuickPickItems`) — asserts the
   label/description/detail are Core's own `shortHash`/`subject`/
   `author.name`/`authorDate` unmodified.

**Result: no divergence found.** All four tests pass against the current
build (`npx vitest run`: 206/206, including this file's 4). This is a
genuinely good outcome, not a shortcut — the two potential hazards this
suite specifically went looking for (`abbreviateHash`'s fixed 8-character
blame truncation vs. Core's own variable-length `%h` short hash; author
field order/formatting) both check out: blame's own 8-character convention
in `blameFormat.ts::abbreviateHash` deliberately mirrors `gitsail-cli`'s own
`output.rs::render_blame` (`&line.commit[..min(8)]`), which is a different,
independently-fixed convention from `CommitDto.short_hash` (Git's `%h`,
variable length) — the two are never confused in the code, and
`historyPresentation.ts` correctly uses `commit.shortHash` (Core's real
field), never `abbreviateHash`, for commit list rows.

**CI-scoping note.** `docs/architecture/ci-policy.md` deliberately runs the
`vscode` CI job with no Rust toolchain (a fast, Node-only job, independent
from the `rust` matrix job). `coreParity.test.ts` needs a real, freshly
built `gitsail-cli` to compare against — that is the entire point — so it
checks `cargo --version` up front and uses `describe.skipIf` to skip itself
where no Rust toolchain is present, rather than turning that job red for an
environment reason unrelated to any extension defect. It runs for real (and
would fail on a genuine divergence) in any environment with a Rust
toolchain, including this session's own verification run. **Recommendation
for a follow-up** (out of this story's scope — changing job topology is a
CI-policy/ADR-022 decision, not a testing-story change): either move this
one file into the `rust` job (it is the one file in `apps/vscode/test` that
needs the Rust toolchain) or add a `cargo build -p gitsail-cli` step ahead
of `npx vitest run` in the `vscode` job.

## Criterion 3 — advanced operations, interruptions, installation, and no forge/network dependency for local flows

### Advanced-operation matrix

| Operation | TUI | Desktop | Core (`gitsail-git`/`gitsail-application`) | VS Code / CLI |
| --- | --- | --- | --- | --- |
| Merge (conflict, resolve, continue, abort) | `tests/merge_conflicts.rs` (5 tests) | `stores/merge.test.ts` + `commands.rs` unit tests (`merge_reports_a_conflict_and_the_full_resolve_continue_flow_completes_it`, `aborting_a_pending_merge_restores_head_and_preserves_unrelated_work`) | `tests/t231_233_merge_conflicts.rs` | N/A — CLI is read-only (see below) |
| Interactive rebase (plan, reorder, reword, squash/fixup/drop, conflict, skip, continue) | `tests/rebase.rs` (9 tests) | `stores/merge.ts` (`requestRebasePlan`/`requestExecuteRebasePlan`) + `RebasePlanPanel.vue`, tested in `stores/merge.test.ts` and `commands.rs` | `tests/t235_237_rebase.rs` | N/A |
| Cherry-pick / revert | `tests/cherry_pick_revert_reset.rs` | `stores/merge.ts` (`requestCherryPick`) | `tests/t238_240_cherry_pick_revert_reset.rs` | N/A |
| Reset (soft/mixed/hard) | `tests/cherry_pick_revert_reset.rs` | `stores/reset.ts`/`reset.test.ts` | `tests/t238_240_cherry_pick_revert_reset.rs` | N/A |
| Amend | `tests/amend.rs` | `stores/amend.ts`/`amend.test.ts` | covered via both above | read side only: `coreParity.test.ts` (this story) |
| Branch create/switch/rename/delete (incl. force-delete) | `tests/branch_management.rs` | `stores/branches.ts`/`branches.test.ts` | `tests/provider.rs` | N/A |
| Patch apply / export | `tests/t163_apply_patch.rs`, `tests/status_and_diff.rs` | `stores/patchApply.test.ts`, `stores/patchExport.test.ts` | `tests/t163_apply_patch.rs` | N/A |
| Remote sync: fetch/pull/push (non-force) | `tests/remote_sync.rs` (7 tests) | `stores/sync.test.ts` | `tests/epic19_remote_operations.rs` | N/A |
| PR/MR listing (forge, read-only) | **not wired** (no TUI panel calls `ListPullRequests` — only `apps/desktop/src-tauri` does) | `stores/pullRequests.ts`/`pullRequests.test.ts` | `gitsail-forge` adapters (`github_pr_adapter.rs`, `gitlab_mr_adapter.rs`) | N/A |
| **Stash** create/apply/pop/drop | listing only (`tests/references_panel.rs`) | listing only | `tests/epic18_stash_tags_worktrees.rs` | not applicable |
| **Tag** create/delete/annotate | listing only (`tests/references_panel.rs`) | listing only | `tests/epic18_stash_tags_worktrees.rs` | not applicable |
| **Worktree** create/remove | not wired | not wired | `tests/epic18_stash_tags_worktrees.rs` | not applicable |
| **Force-push (with lease)** | not wired (`gitsail-tui`'s T-182 `OperationKind::Push` "deliberately excludes it", per `commands.rs:1107`'s own comment) | explicitly out of scope, by design (`commands.rs:1103-1107`: "a rejected push must never be silently escalated to a force push... `force_push_with_lease`'s own, separate, out-of-scope operation") | `tests/epic19_remote_operations.rs` | not applicable |

**Reading the gaps (bold rows).** Stash/tag/worktree *mutation* and
force-push are fully implemented and tested at the Core layer
(EPIC-18/EPIC-19), but no interface currently exposes them as a *mutating*
UI action — only read-only listing exists in TUI/Desktop (stash entries,
tags, remotes), and the CLI (`gitsail-cli`) is a deliberately read-only
"six query commands" tool by design (`crates/gitsail-cli/src/cli.rs`'s own
doc comment: "Runs one read-only query"), so it was never a candidate to
wire mutations into either. This is a **pre-existing product-scope
decision from earlier epics** (matches this project's own epic-dependency
tracking: these UI stories are separate, later backlog items, not part of
EPIC-24), not a testing gap this story can close by writing more tests —
there is no UI code path to test yet. Escalated here explicitly per this
story's own DoD ("se... fora de escopo, escalar claramente... com
justificativa") rather than silently treated as "already covered" because
Core has tests.

### Interruption matrix

| Scenario | TUI | Desktop | VS Code |
| --- | --- | --- | --- |
| Decline before confirmation (the product's only "cancel" semantics — Destructive Operations & Confirmation Guardrails rule 5: "cancelling before confirmation leaves the repository completely untouched") | `merge_conflicts.rs::declining_a_pending_merge_confirmation_dispatches_nothing_and_leaves_head_untouched`; `rebase.rs::escaping_the_reword_prompt_discards_only_the_unsubmitted_text`, `::squash_on_the_first_entry_is_refused_before_confirming` | `operation.test.ts` (decline-before-confirm) | QuickPick Escape dismissal — `historyController.test.ts` (file history and, per T-255's audit, line history is the identical shared branch, deliberately not duplicated) |
| Stale precondition / HEAD moved between preview and confirm (`expected_head` revalidation) | `app.rs::tests::a_failed_amend_from_a_stale_head_preserves_the_message_and_never_refreshes`; `rebase.rs::a_stale_plan_is_refused_clearly_rather_than_silently_rebuilt` | `commands.rs::amend_commit_reports_a_conflict_when_the_write_port_refuses_a_stale_head`; `run_mutation`'s epoch guard (`commands.rs:64-79`) | N/A (extension never mutates) |
| Starting a new operation while one is already pending | (guarded at Core level, exercised via `merge`/`rebase` conflict tests above) | `commands.rs::merging_while_another_operation_is_pending_is_refused` | N/A |
| CLI/process timeout | `gitsail-git`'s `ProcessRequest::with_timeout`/`wait_with_timeout` (Core-level, shared by every interface's `GitProcessRunner`); `gitsail-cli`'s own `--timeout` flag (`cli.rs`) | same shared Core mechanism | `cliClient.test.ts::rejects with CliTimeoutError and stops waiting once the timeout elapses` — the extension's own outer, client-side guard around the spawned process |
| Mid-flight cancellation of a running operation | not implemented (by design — `worker.rs`'s own doc: "cancellation... is out of scope... each read is issued with a fresh, never-cancelled `CancellationToken`") | not implemented (T-255's audit: "no such feature exists... GitSail's only cancellation semantics anywhere in this product is decline-before-confirm") | `AbortSignal` accepted by `GitSailCliClient.run` and tested (`cliClient.test.ts`), but `historyController.ts` never plumbs one into a blame/history call — a known, already-escalated gap (T-255's own audit: "a potential product gap, not a test gap... out of scope for a testing story") |

No new blocking defect found in this matrix; the two "not implemented"
rows above are pre-existing, already-documented product scope decisions
(cited to their own prior audit/code comments), not silently-swallowed
bugs.

### Installation matrix

| Concern | Coverage |
| --- | --- |
| VS Code: locating/validating the separately-installed `gitsail` CLI binary (not found, incompatible version, unrecognized version output, workspace-trust gating of a configured path) | `cliLocator.test.ts` (16 tests across 5 `describe` blocks) |
| `git` itself missing or unrecognized (`ErrorCode::GitNotInstalled`) — the one installation dependency shared by *every* interface, since TUI/Desktop link Core directly and VS Code's CLI in turn shells out to `git` | **Gap found and closed this story**: no test anywhere in the workspace previously exercised `GitExecutable::discover`/`GitProcessRunner::new` actually returning `GitNotInstalled` for a missing/misconfigured `git` path — every other test always ran on a machine with a real `git` on `PATH`. Closed: `crates/gitsail-git/src/runner.rs::tests::discover_reports_git_not_installed_for_a_nonexistent_override_path` and `::runner_new_reports_git_not_installed_for_a_configured_nonexistent_path` (new) |
| `gitsail-cli`'s exit code for `GitNotInstalled`/`UnsupportedGitVersion` | `crates/gitsail-cli/src/exit_code.rs` (existing tests) |
| Desktop/TUI binary packaging across ubuntu/windows/macos | Out of scope for this story (a packaging/CI concern, not a Git-behavior concern) — already covered structurally by T-254/US-121's multiplatform `rust` CI matrix (`ci-policy.md`), which builds `apps/desktop/src-tauri` and `gitsail-tui` on all three OS legs. |

### No forge-account / network dependency for local Git flows (T-244/T-245 re-audit)

**Claim to verify:** no local Git flow ever makes a network call, and
nothing requires a connected forge account/token before a local operation
works.

**Audit method:** every crate's `Cargo.toml` was inspected for an HTTP
client dependency, then every call site of the one dependency found was
checked for how it is gated.

- `ureq` (the only HTTP-capable dependency anywhere in this workspace —
  confirmed by inspecting every `Cargo.toml`; `crates/gitsail-forge/Cargo.toml`'s
  own comment records the same "grepped the whole workspace for
  `reqwest`/`tokio`/`hyper`; none were present" check from when it was
  added) is a dependency of exactly one crate: `gitsail-forge`.
- `gitsail-cli` (`crates/gitsail-cli/Cargo.toml`) and `gitsail-tui`
  (`crates/gitsail-tui/Cargo.toml`) do not depend on `gitsail-forge` at
  all, not even transitively — the CLI and TUI binaries are **physically
  incapable** of making an HTTP call; the capability is absent from the
  compiled binary, not merely unused.
- Only `apps/desktop/src-tauri` depends on `gitsail-forge`
  (`Cargo.toml:.../gitsail-forge`), and the only place it is invoked is the
  single, explicit `list_pull_requests` Tauri command
  (`commands.rs:941-957`) — never from `open_repository`, `status`, or any
  mutation path. `ConnectForgeAccount`/`ForgeCredentialPort`
  (`forge_credentials.rs`) itself never calls the network either, by its
  own documented scope cut: "stores whatever token it is given without
  reaching out to the network."
- `git fetch`/`pull`/`push` themselves do reach a remote, but that is Git's
  own transport to a repository's configured remote — not a GitSail forge
  account/token, and every one of `remote_sync.rs`/`sync.test.ts`/
  `epic19_remote_operations.rs`'s tests runs against a local, on-disk bare
  remote with no forge/network/token involved at all (`fetch_with_no_remote_configured_shows_a_clear_error_instead_of_guessing`
  in `remote_sync.rs` additionally proves a repository with **no** remote
  configured still degrades to a clear, local error rather than attempting
  any network reach).

**Conclusion: no defect found.** This holds exactly as designed by
`security-privacy-credentials-rules` rule 6 ("GitSail must work entirely on
local repositories without any central GitSail account") and
`github-gitlab-integration-rules` rule 1 ("Local Git functionality never
depends on a connected GitHub/GitLab account").

## Verification

Run from this checkout:

- `cargo test --workspace` — all green (50 test binaries, including the new
  `tui_desktop_parity` (2 tests) and `gitsail-git`'s two new
  `GitNotInstalled` tests).
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `cargo fmt --all -- --check` — clean.
- `apps/desktop`: `npm run test -- --run` — 41 files, 300 tests, all green
  (unchanged by this story); `npm run build` — clean.
- `apps/vscode`: `npx vitest run` — 24 files, 206 tests, all green
  (including the new `coreParity.test.ts`, 4 tests); `npx tsc -p ./` —
  clean.

## Files changed

- `crates/gitsail-application/tests/tui_desktop_parity.rs` (new) — criterion
  1's comparative parity tests.
- `crates/gitsail-application/Cargo.toml` — added `gitsail-test-support` as
  a dev-dependency.
- `apps/vscode/test/coreParity.test.ts` (new) — criterion 2's Core-vs-extension
  parity tests.
- `crates/gitsail-git/src/runner.rs` — two new `GitNotInstalled` tests
  (criterion 3's installation-matrix gap, closed).
- `docs/architecture/integrated-regression-report-us123.md` (this file).

## Escalated, out-of-scope items (not blocking)

1. Stash/tag/worktree *mutation* and force-push have no UI wiring in
   TUI/Desktop yet (Core-only, fully tested there). Pre-existing
   product-scope decision from earlier epics, not something this testing
   story can close by writing more tests.
2. `historyController.ts` never plumbs an `AbortSignal` into a blame/history
   CLI call. Already escalated by T-255's own audit as a potential product
   gap, reconfirmed here, still out of scope for a testing story.
3. `coreParity.test.ts` needs a Rust toolchain and therefore skips itself in
   the `vscode` CI job as currently scoped (no Rust toolchain there, per
   `ci-policy.md`). Recommend either moving it into the `rust` job or adding
   a `cargo build -p gitsail-cli` step to the `vscode` job — a CI-topology
   decision outside this story's scope to make unilaterally.

With this report, EPIC-24 (Testing & Quality) is complete: T-256/US-123 is
the epic's last task.
