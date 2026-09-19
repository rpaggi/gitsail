# Contributing to GitSail

Thanks for considering a contribution to GitSail. This document explains how
to set up the project, run its tests, respect its architecture boundaries,
and get a change reviewed and merged. It is written in English, like all
primary technical documentation in this repository (`AGENTS.md`); the
product backlog and PRD stay in Portuguese for the product owner and are not
affected by this process.

## 1. Setup

### Prerequisites

See the README's [Requirements](README.md#requirements) section for the
exact versions: Git ≥ 2.31, Rust ≥ 1.97.0 (the workspace's pinned MSRV,
ADR-021), and Node.js 18+/npm only if you touch `apps/desktop` or
`apps/vscode`. All of the commands below were run against this exact
source tree while writing this document (2026-09-18), on Linux, with Git
2.43.0, `cargo`/`rustc` 1.97.0, Node v22.22.3, npm 10.9.8 — every one
finished with the outcome stated next to it.

### Get the code and build it

```sh
git clone git@github.com:rpaggi/gitsail.git
cd gitsail
cargo check --workspace     # fast type/borrow check of every Rust crate
cargo build --workspace     # full build, all crates including apps/desktop/src-tauri
```

`cargo build --release -p gitsail-cli` produces the CLI binary at
`target/release/gitsail` (see the README's CLI section for example
invocations).

### Frontend apps

```sh
cd apps/desktop && npm install    # Vue 3 + Tauri v2 desktop frontend
cd apps/vscode  && npm install    # VS Code extension
```

On Linux, `apps/desktop`'s Rust backend (a Tauri v2 app, workspace member
`apps/desktop/src-tauri`) additionally needs the system WebKitGTK/GTK/D-Bus
development headers Tauri links against — see
`.github/workflows/ci.yml`'s `Install Linux system dependencies` step for
the exact package list this repository's own CI installs, if your
distribution doesn't already have them.

## 2. Tests

This is exactly what `.github/workflows/ci.yml` runs (see
`docs/architecture/ci-policy.md` for which of these are required for merge
vs. informative-only) — run the same commands locally before opening a PR:

```sh
cargo fmt --all -- --check                              # formatting
cargo clippy --workspace --all-targets -- -D warnings    # lints, zero warnings allowed
cargo test --workspace                                   # every Rust crate's tests
cargo build --workspace                                  # release-shape build check

cd apps/desktop && npm run test -- --run && npm run build   # Vitest + vue-tsc/vite
cd apps/vscode  && npx vitest run && npx tsc -p ./           # Vitest + tsc
```

All of the above were run in full while writing this document and passed,
with one exception worth knowing about: under `cargo test --workspace`'s
full parallel run,
`gitsail-git`'s `line_history_fails_with_cancelled_when_the_token_is_already_cancelled`
(a cooperative-cancellation timing test in
`crates/gitsail-git/tests/provider.rs`) failed once under contention from
the rest of the suite running concurrently, but passed on every one of
three immediate reruns in isolation (`cargo test -p gitsail-git --test
provider line_history_fails_with_cancelled_when_the_token_is_already_cancelled`).
Treat a single failure of *that specific test* under `--workspace` as a
known, pre-existing flake, not a regression in your change — but do treat a
new or differently-named failure as real. If you have time to dig into
*why* it's flaky (likely a cancellation-vs-completion race rather than a
correctness bug), a fix is welcome.

Also run the architecture fitness checks locally — the same script CI runs:

```sh
bash scripts/ci/check-architecture.sh
```

### Writing new integration tests: `gitsail-test-support`

If you're adding a `gitsail-git` (or `gitsail-tui`/`gitsail-cli`)
integration test that needs a real, disposable Git repository, use the
`gitsail-test-support` crate (`crates/gitsail-test-support`, T-252/US-119)
instead of hand-rolling a temp-dir-plus-`git`-subprocess helper:

```rust
use gitsail_test_support::Fixture;

#[test]
fn my_new_behavior() {
    let fixture = Fixture::with_history(); // or ::empty_repo(), ::with_conflict(), etc.
    // fixture.path(), fixture.state (a FixtureState) describe exactly what
    // was built, so the test asserts against a declared contract instead
    // of re-deriving expectations from the setup code.
}
```

It builds a real, isolated repository per `Fixture` (never simulated,
recursively cleaned up on drop) and already covers empty/bare/shallow
repos, unborn/detached `HEAD`, merge/rebase conflicts, renames, and binary
files — see `crates/gitsail-test-support/src/fixture.rs`'s module docs and
each constructor's own doc comment for the full list before writing a new
one from scratch. Note: not every existing integration test file has
migrated to this crate yet (see that module's doc for which ~2 of ~10 have)
— migrating one you're already touching is a welcome, non-blocking cleanup,
but not a prerequisite for an unrelated change.

## 3. Architecture boundaries

GitSail follows Ports & Adapters (Hexagonal Architecture, ADR-002). Before
changing production code, read the applicable section of
`docs/architecture/GitSail_SAD_and_ADRs_v0.1.md` (its top-of-file ADR index
links straight to the relevant decision) and, in particular:

- **The domain never depends on infrastructure, application, or
  presentation code.** `crates/gitsail-domain` must not declare a
  `[dependencies]` entry on any other GitSail workspace crate. This is
  enforced mechanically, not just by convention — see
  `scripts/ci/check-architecture.sh`'s first check, which fails CI's
  `architecture-fitness` job if this is ever violated, and run it locally
  (§2 above) before pushing.
- **Only `gitsail-git` invokes the real `git` binary.** No `*.rs` file
  outside `crates/gitsail-git/**` (with the narrow, documented exceptions
  in the script's own comments: `gitsail-test-support`, `**/tests/**.rs`
  integration binaries, and one inline `#[cfg(test)]` block in
  `apps/desktop/src-tauri/src/commands.rs`) may construct a subprocess
  literally named `"git"`. This is the script's second check.
- **Dependencies point inward:** Presentation (CLI/TUI/Desktop/VS Code) →
  Application → Domain, with Ports as the interface boundary Application
  depends on and Adapters (e.g. `GitCliProvider`) implement. A new Git
  provider only needs to satisfy the existing ports and pass the provider
  contract tests (`gitsail-test-support::contract`) — it should never
  require changing `gitsail-domain` or `gitsail-application`.

`docs/architecture/ci-policy.md` is the operational companion to these
rules: it states plainly what each CI job checks, which are blocking vs.
informative, and the reasoning behind that split (e.g. why
`cargo audit`/`npm audit` start informative-only).

## 4. Code style

- `cargo fmt` formatting is required (`cargo fmt --all -- --check` in CI).
- `cargo clippy --workspace --all-targets -- -D warnings` must be clean —
  the workspace forbids `unsafe_code` outright (`[workspace.lints.rust]` in
  the root `Cargo.toml`) and treats every clippy warning as an error in CI.
- Prefer small, focused commits with descriptive messages. This repository
  has no enforced commit-message template beyond describing *why* a change
  was made, not just *what* changed.

## 5. Pull request / review process

- Open an issue first for anything beyond a small, obvious fix, using the
  templates under `.github/ISSUE_TEMPLATE/` — they ask you to name which
  backlog User Story/Epic (e.g. `US-037`, `EPIC-08`) or Takumi task
  (`T-###`) the change relates to, if any, and for reproduction steps.
  **Never paste a token, password, or credential-bearing URL into an issue
  or PR** — redact it first; this is a hard rule (see
  `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`'s Security/Privacy
  material, ADR-010, ADR-017/ADR-018) and reused verbatim in
  `.github/ISSUE_TEMPLATE/` and `.github/PULL_REQUEST_TEMPLATE.md`.
- Use `.github/PULL_REQUEST_TEMPLATE.md`: it asks for the related
  US/EPIC/task ID, a summary of the change, which of §2's test commands you
  ran locally, and confirmation that no secret/credential is included
  anywhere in the diff or its description.
- CI (`.github/workflows/ci.yml`) must pass on the required jobs listed in
  `docs/architecture/ci-policy.md` (`rust` on all three OSes, `desktop`,
  `vscode`, `architecture-fitness`) before merge; `dependency-audit` is
  informative only for now.
- A reviewer checks: does the change stay inside its layer's boundary
  (§3 above)? Is it tested at the level that actually exercises it (a
  `gitsail-git` behavior change needs a `gitsail-git` integration test, not
  just a CLI-level one)? Does it need a new or updated ADR (§6 below)?

## 6. Updating PRD/SAD/ADRs

The PRD (`docs/product/GitSail_PRD_v0.1-v1.0.md`), the SAD, and its ADRs
(both in `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`) are living
documents, but their identifiers and decision history are permanent record,
not a draft to rewrite. Rules, in order of how often they come up:

1. **Never reuse an ADR number.** Once `ADR-0NN` has been assigned to a
   decision, that number is that decision's, forever — even if the decision
   is later revised, superseded, or found to be wrong. Do not renumber
   existing ADRs to close a gap, and do not assign a used number to a new,
   unrelated decision.
2. **Revising a decision edits the ADR in place, preserving its number.**
   Add a dated revision note inside that same ADR (e.g. a `### Revision —
   2026-MM-DD` subsection under its existing `### Consequences`) explaining
   what changed and why, rather than deleting or silently rewriting the
   original Context/Decision text. If a decision is fully superseded,
   change its `**Status:**` line (e.g. `Accepted` → `Superseded by
   ADR-0NN`) and add the revision note — never delete the ADR.
3. **A new decision gets the next unused ADR number**, appended after the
   current last one (today: `ADR-026` is next, since `ADR-001`–`ADR-025`
   are all taken — see the ADR index at the top of the SAD file, which
   must be kept in sync whenever an ADR is added or its status changes).
4. **US/EPIC/T-xxx IDs in the backlog and PRD follow the same rule**: never
   renumbered or reused, even if a story is cancelled or split. Add a
   status note instead of deleting the entry.
5. **If the SAD/ADR document is ever split** into multiple files (e.g. one
   file per ADR, or the SAD separated from its ADRs), every section's and
   every ADR's original text must be preserved verbatim in the split — this
   is not a rewrite opportunity. The ADR index at the top of the current
   single-file SAD exists partly so that such a split has a ready-made
   table of contents to carry over unchanged.
6. **Technical sections stay in English**; only the Portuguese
   product-planning documents (backlog, and any PRD content already in
   Portuguese) are exempt, per `AGENTS.md`.

If your change touches `gitsail_protocol::SCHEMA_VERSION` or a component's
supported-version list, also update
`docs/architecture/protocol-compatibility.md` in the same change — ADR-016
requires that document to stay current with the code, not just with the
ADR that established the policy.

## 7. Releases

`.github/workflows/release.yml` (ADR-023) is a separate pipeline from
`ci.yml` — it never runs on a pull request and is not part of the required
merge checks in `ci-policy.md`. It only runs on a pushed `vX.Y.Z` tag (or a
manual dispatch against an existing one) and publishes CLI/TUI archives,
Desktop installers, and a VS Code `.vsix` to a GitHub Release. See
`docs/architecture/release-process.md` for what it builds, the registered
decision to distribute via GitHub Releases only for now (no code
signing/notarization, no Marketplace/Open VSX publishing yet), and exactly
what changes once those become available.
