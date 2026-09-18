# GitSail CI policy (T-254/US-121)

This document is the operational companion to ADR-022 (`GitSail_SAD_and_ADRs_v0.1.md`).
It states, concretely, what `.github/workflows/ci.yml` runs, which of its
checks are **required for merge** vs. **informative only**, and the
assumptions/limitations behind that first pipeline (there was no CI in this
repository before T-254 — `.github/workflows/` did not exist).

This document does not cover `.github/workflows/release.yml` (ADR-023,
tag-triggered, publishes GitHub Release artifacts — never runs on a pull
request and is not one of the required-for-merge checks below) — see
`docs/architecture/release-process.md` for that pipeline's own policy.

## Jobs and what they check

| Job | Runs on | Purpose |
| --- | --- | --- |
| `rust` | ubuntu-latest, windows-latest, macos-latest (matrix) | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --workspace` — the whole Rust workspace: `gitsail-domain`, `gitsail-application`, `gitsail-git`, `gitsail-protocol`, `gitsail-cli` (the mandatory v0.1 baseline), plus `gitsail-tui`, `gitsail-test-support` and `apps/desktop/src-tauri` (already regular `[workspace] members` in the root `Cargo.toml`, so they build/test in this same job for free). |
| `desktop` | ubuntu-latest only | `apps/desktop` frontend: `npm ci`, `npm run test -- --run` (Vitest), `npm run build` (`vue-tsc --noEmit && vite build`). |
| `vscode` | ubuntu-latest only | `apps/vscode` extension: `npm ci`, `npx vitest run`, `npx tsc -p ./`. |
| `architecture-fitness` | ubuntu-latest only | Runs `scripts/ci/check-architecture.sh` — see below. |
| `dependency-audit` | ubuntu-latest only | `cargo audit`, `npm audit` (both apps) — **informative only**, see below. |

## Required vs. informative checks

**Required for merge (must be green):**
- `rust` on all three matrix legs (ubuntu-latest, windows-latest, macos-latest).
- `desktop`.
- `vscode`.
- `architecture-fitness`.

**Informative only, does not block a merge:**
- `dependency-audit` (every step has `continue-on-error: true`).

This split is a v0.1-launch decision, not a permanent one (see "Evolution"
below). Branch protection on `main` should list the required jobs above by
name once this workflow's first run has produced check names for GitHub to
offer in that settings UI (this cannot be configured from inside this
sandbox — no live GitHub Actions/API access here, see "Sandbox
verification limits").

### Why `dependency-audit` starts informative-only

This is the *first* CI pipeline this project has ever had. Turning
`cargo audit`/`npm audit` into a hard merge gate on day one has two
concrete costs that outweigh the benefit right now:
1. Advisory databases (RustSec, the npm advisory feed) can flag a
   transitive dependency with no available fix yet, or one irrelevant to
   how GitSail actually uses it (e.g. an advisory in a feature GitSail never
   enables) — a hard gate would then block unrelated, correct changes on a
   problem nobody has triaged.
2. Nobody has yet reviewed what today's dependency tree actually reports.
   Flipping this to blocking before that first triage would likely fail the
   very first run for reasons unrelated to whoever's PR happens to trigger
   it.

The audit jobs are wired in now (not deferred) specifically so that first
triage can happen — the reports are visible in every CI run's logs — and
`ci-policy.md`/this section is the record of *why* they don't block yet, so
this is a deliberate, revisited decision, not silent neglect.

## Architectural fitness functions (`scripts/ci/check-architecture.sh`)

Approximate, grep/text-based checks — not a full architectural-conformance
tool. Two checks today, corresponding to US-121 criterion 2:

1. **Domain isolation.** `crates/gitsail-domain/Cargo.toml` must not declare
   a `[dependencies]` entry on any other GitSail workspace crate
   (`gitsail-git`, `gitsail-application`, `gitsail-cli`, `gitsail-forge`,
   `gitsail-tui`, `gitsail-protocol`, `gitsail-test-support`). Verified
   today (2026-09-18): `gitsail-domain`'s only dependency is the external
   `url` crate; `cargo tree -p gitsail-domain` confirms no GitSail crate
   appears in its dependency graph.
2. **Restricted Git parsing.** No `*.rs` file outside
   `crates/gitsail-git/**` may construct a subprocess literally named
   `"git"` (`Command::new("git")`). The check is narrow on purpose — it
   matches the literal string `"git"`, not every `std::process::Command`
   use — because legitimate, unrelated subprocess use exists elsewhere
   (e.g. `gitsail-tui/src/browser.rs` and
   `apps/desktop/src-tauri/src/browser.rs` shell out to the OS's URL opener,
   `xdg-open`/`open`/`cmd`, never to `git`). It has three documented,
   reviewed exceptions — `crates/gitsail-test-support/**` (a dev-dependency
   -only fixture crate, T-252/US-119), any `**/tests/**.rs` integration test
   binary, and `apps/desktop/src-tauri/src/commands.rs`'s inline
   `#[cfg(test)] mod tests { mod remote_sync_real_git { ... } }` block —
   see the script's own comments for the full rationale of each, including
   the known limitation that a plain grep cannot see `#[cfg(test)]`
   boundaries inside that one allowlisted file.

The third criterion-2 check — an **explicit protocol** — is *not*
reimplemented here: `SCHEMA_VERSION`/compatibility tests already exist from
T-172/ADR-016 (`crates/gitsail-protocol/tests/compatibility.rs`,
`crates/gitsail-protocol/tests/dto_wire_shape.rs`,
`crates/gitsail-cli/tests/protocol_compatibility.rs`) and already run as
ordinary tests inside the `rust` job's `cargo test --workspace` step. No
separate job reruns them.

## Assumptions

- **GitHub-hosted runners ship `git` already.** `ubuntu-latest`,
  `windows-latest` and `macos-latest` all come with a recent `git`
  preinstalled (documented in GitHub's own `runner-images` tool manifests);
  this pipeline relies on that and does not install `git` itself. If GitHub
  ever drops this from a runner image, every `gitsail-git` integration test
  (which shell out to a real `git` subprocess) would need this step added
  back explicitly.
- **Tauri v2 needs no extra system packages on Windows/macOS.** Only Linux
  needs the WebKitGTK/GTK/AppIndicator/etc. package list installed in the
  `rust` job's conditional step; Tauri's own v2 prerequisites documentation
  states Windows needs the Rust toolchain + WebView2 (already present on
  current Windows runner images) and macOS needs Rust + Xcode Command Line
  Tools (already present on macOS runner images) — no `apt`/`brew`/`choco`
  install step is added for either. This is a documented assumption, not
  independently verified against a live Windows/macOS runner from this
  sandbox (see below).

## Sandbox verification limits

This pipeline could not be executed against real GitHub Actions from the
environment T-254 was implemented in — no GitHub Actions access, no
Windows/macOS runners, nothing that can trigger a live workflow run. What
*was* verified locally, on Linux, before this file existed:
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace`, `cargo build --workspace` — all
  run directly, all green.
- `apps/desktop`: `npm run test -- --run` and `npm run build` — both run
  directly, both green.
- `apps/vscode`: `npx vitest run` and `npx tsc -p ./` — both run directly,
  both green.
- `scripts/ci/check-architecture.sh` — run directly (passes), and verified
  against a deliberately planted violation (a throwaway file calling
  `Command::new("git")` outside `crates/gitsail-git`) to confirm it actually
  fails when it should, not just when it happens to.
- `.github/workflows/ci.yml`'s YAML syntax — validated two ways:
  `python3 -c "import yaml; yaml.safe_load(open(...))"` (parses cleanly),
  and `actionlint` (installed via `go install
  github.com/rhysd/actionlint/cmd/actionlint@latest` for this one-time
  check, since neither a package nor a prebuilt binary was already present
  in this sandbox) — zero findings.

None of the above substitutes for an actual green run on
`windows-latest`/`macos-latest`/`ubuntu-latest` through GitHub Actions
itself, which only happens once this workflow is pushed. This is the same
class of limitation already recorded elsewhere in this project for other
environments this sandbox cannot reach (Tauri/VS Code end-to-end runs, a
real OS keyring).

## Evolution

US-121 criterion 3 ("pipeline evolves with TUI, Desktop, extension and a
dependency audit as they arrive") is already satisfied as of this first
version, not deferred: those components already exist in this repository
today (EPIC-09/10/11 for TUI/Desktop foundations, the VS Code extension
under `apps/vscode`), so the `rust` matrix already builds/tests TUI and
Desktop's Rust side, and `desktop`/`vscode` are their own jobs from this
pipeline's first version. Future evolution this document anticipates:
- Promote `dependency-audit` from informative to blocking once the current
  dependency tree has had its first real triage (see "Why
  `dependency-audit` starts informative-only" above).
- Add Windows/macOS legs to `desktop`/`vscode` if/when a genuinely
  OS-specific behavior in either needs its own coverage (none is known to
  exist yet).
- Revisit the Rust MSRV pin (`RUST_TOOLCHAIN` in `ci.yml`, mirroring
  `rust-version` in the root `Cargo.toml`) once ADR-021's floor-equals-current
  pin gets tested against an actually older toolchain — ADR-021 explicitly
  flagged that as something CI existing would make possible.
