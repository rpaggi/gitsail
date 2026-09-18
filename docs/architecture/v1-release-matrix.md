# GitSail v1.0 release matrix and checklist (T-261/US-128)

## What this document is, and is not

**This is a reference matrix and checklist to be filled with real values at
the moment of the actual v1.0 release — it is not an announcement that
v1.0 has shipped.** As of this writing (2026-09-18):

- No `vX.Y.Z` tag — and in particular no `v1.0.0` tag — has been pushed
  against this repository. `.github/workflows/release.yml`
  (`docs/architecture/release-process.md`) is ready to build and publish a
  GitHub Release the moment one is pushed, but has never actually run
  against a real tag.
- Every workspace crate, `apps/desktop/src-tauri`'s `Cargo.toml`/
  `tauri.conf.json`, and `apps/vscode/package.json` are pinned to version
  `"0.0.0"` (ADR-021, pre-1.0 versioning policy) — this is deliberate and
  does not change until a real release is cut. The "version" column below
  therefore reports that pinned identifier, not a release number, except
  where noted.
- No date, tag, or version number is invented anywhere in this document.

This document does two things: (1) a component/version/schema/platform
matrix (§1), and (2) the v1.0 readiness checklist the backlog's own gate
(`docs/product/GitSail_Product_Backlog_v1.0.md` §5: "Todos os Must até v1.0
aceitos, decisões Should/Could registradas e US-128 + US-133 completos")
asks for, built from the real Takumi board and cross-checked against the
backlog document — not from memory or assumption (§3–§4).

## 1. Component / version / schema / platform matrix

| Component | Current version identifier | `schemaVersion` supported | Platforms built & tested | Status |
|---|---|---|---|---|
| **Core** (`gitsail-domain`, `gitsail-application`, `gitsail-git`, `gitsail-protocol`, `gitsail-forge`) | `0.0.0`, pinned workspace-wide (ADR-021) | Defines and produces `SCHEMA_VERSION = 1` (`gitsail-protocol`); consumed by CLI (producer role) and VS Code (consumer role) | Built + unit/integration-tested on Linux, Windows, and macOS via the `rust` CI matrix job (`docs/architecture/ci-policy.md`) | Delivered — EPIC-01–EPIC-08, EPIC-16–EPIC-19, EPIC-22–EPIC-23 Must stories are `closed`/`review` in Takumi (§3) |
| **CLI** (`gitsail-cli`) | `0.0.0` pinned; the release archive's `VERSION.txt` and filename carry the real tag once one is cut (`release-process.md`) | Produces `schemaVersion: 1` (only component that writes an `Envelope` today) | Built + tested on Linux/Windows/macOS in CI; release archives for `linux-x86_64`, `windows-x86_64`, `macos-aarch64` — **macOS Intel (x86_64) is not built yet** (known gap, `release-process.md`) | Delivered — T-257/US-124 in `review` |
| **TUI** (`gitsail-tui`) | `0.0.0` pinned; ships inside the same CLI/TUI archive as the CLI | Not applicable — no wire boundary; compiled with `gitsail-protocol` in the same build, so Rust's type system is the compatibility check (`protocol-compatibility.md`) | Same platforms/archives as CLI above | Delivered — T-257/US-124 in `review`; EPIC-09, EPIC-10, and the TUI side of EPIC-17 are `closed`/`review` |
| **Desktop** (`apps/desktop`) | `0.0.0` pinned in `tauri.conf.json`/`Cargo.toml`; the installer's *displayed* version is stamped from the release tag at build time only, never committed (`release-process.md`) | Not applicable on the wire — Tauri IPC commands return `gitsail-protocol` DTOs directly to the Vue webview; no `Envelope` boundary exists here (`protocol-compatibility.md`) | Rust backend built + tested on Linux/Windows/macOS in CI; frontend (Vitest/build) tested on `ubuntu-latest` only. Packaging: Linux `.deb` built and inspected successfully; `.AppImage` was attempted but hit a sandbox-only (WSL2) failure, unverified against a real runner; Windows `.msi`/NSIS and macOS `.dmg`/`.app` were **not attempted at all** — no Windows/macOS host was available in the environment this pipeline was built in (`release-process.md`'s "Known gaps") | Delivered — T-258/US-125 in `review`; EPIC-11, EPIC-12, EPIC-13, and the Desktop side of EPIC-16/EPIC-17 are `closed`/`review`. No code signing/notarization (registered decision, ADR-023) |
| **VS Code extension** (`apps/vscode`) | `0.0.0` pinned in `package.json`; the packaged `.vsix` is stamped with the release tag at package time only, never committed | Consumes `SUPPORTED_SCHEMA_VERSIONS = [1]` (`apps/vscode/src/protocol.ts`) | Packaged and tested on `ubuntu-latest` only — the extension itself is platform-independent JS/TS; it depends on a separately installed `gitsail` CLI matching the user's own OS (`cliLocator.ts`) | Delivered — T-259/US-126 in `review`; EPIC-14 and EPIC-15 are `closed`/`review`. **Not published** to the VS Code Marketplace or Open VSX (registered decision, ADR-023) — the `.vsix` attached to the GitHub Release is the official install path today |

## 2. Where the rest of the release evidence lives (linked, not duplicated)

- **Artifacts, checksums, and versioning mechanics** — `docs/architecture/release-process.md` (what `.github/workflows/release.yml` builds, `SHA256SUMS.txt`, the code-signing/notarization and Marketplace/Open VSX decisions, and the exact "known gaps" list this matrix's platform column summarizes).
- **Release notes boilerplate every tag reuses** — `docs/architecture/release-notes-template.md`.
- **`schemaVersion` / protocol compatibility contract** — `docs/architecture/protocol-compatibility.md`.
- **Update checking** (Desktop only, check-only, never an auto-installer) — `docs/architecture/update-mechanism.md`.
- **Troubleshooting, credentials, configuration files, privacy** — `docs/manual/troubleshooting.md`.
- **What is deliberately deferred or genuinely still open** (read this before promising a capability) — `docs/manual/roadmap-and-open-decisions.md`.
- **Performance measurement** — `docs/architecture/performance-baseline.md`.
- **Cross-platform / cross-interface regression evidence** — `docs/architecture/integrated-regression-report-us123.md`, `docs/architecture/test-coverage-audit-us122.md`.
- **Preferences consolidation across interfaces** — `docs/architecture/preferences-matrix.md`.

## 3. v1.0 gate checklist — Must stories

The backlog's v1.0 gate (`GitSail_Product_Backlog_v1.0.md` §5) requires
**every** Must story from v0.1 through v1.0 to be accepted, not only the
ones whose "Versão alvo" reads `v1.0`. Of the backlog's 133 stories, 128
are Must priority.

**Evidence method:** an exhaustive sweep of the Takumi board's `todo` and
`review` columns (not a sample), cross-checked against the backlog's own
MoSCoW/milestone table. As of this audit, Takumi's `todo` column contains
exactly **three** tasks in total: T-147/US-014 (Could), T-246/US-104
(Could), and T-261/US-128 (Must — this very story, resolved by this
document). No other Must-priority task anywhere on the board is in `todo`;
everything else is `closed` or `review` (`done` is unused in this
project's workflow — `closed` is this board's terminal state for both
accepted work and consciously-registered exceptions, see §4).

### 3.1 Must pending that blocks v1.0

**None outstanding as a real implementation gap.** The one Must-priority
task that was in `todo` (T-261/US-128) is the checklist this document
delivers.

Two data-integrity issues surfaced during this audit, and both were
**corrections of stale tracking, not implementation gaps** — recorded here
rather than quietly fixed and hidden:

1. **T-162 (US-029, Must, "Copy or export patch", EPIC-06) and T-163
   (US-030, Should, "Apply patch when supported", EPIC-06) carried a stale
   completion note.** Earlier in this session, both tasks were closed with
   a note reading "Explicitamente adiada, não iniciada" (explicitly
   deferred, not started), blocked on EPIC-09/EPIC-11 (and, for T-163,
   also EPIC-22/EPIC-23) not existing on the board yet. All of those epics
   have since landed — and, contrary to that stale note, both stories
   **were, in fact, implemented afterward**: commit `a3b0e5d` ("Implement
   T-162 (US-029) — Copy or export patch") and commit `dfa62e8` ("Implement
   T-163 (US-030) — Apply patch when supported"). Real code
   (`gitsail-application/src/patch.rs`'s `export_patch`,
   `gitsail-tui/src/clipboard.rs`, `apps/desktop/src/stores/patchExport.ts`
   / `patchApply.ts`, `PatchApplyPanel.vue`) and tests
   (`crates/gitsail-git/tests/t163_apply_patch.rs`,
   `stores/patchApply.test.ts`, `stores/patchExport.test.ts`) exist and are
   independently confirmed by `docs/architecture/integrated-regression-report-us123.md`'s
   advanced-operation matrix ("Patch apply / export" row). Both Takumi
   tasks' descriptions were updated by this task to append a correction
   note rather than leave the outdated "not started" claim standing next
   to a `closed` status — no functional gap, a tracking hygiene issue, now
   fixed.
2. **`docs/manual/roadmap-and-open-decisions.md` listed T-195/US-062**
   ("Query blame, tags, stash and remotes" in Desktop) **under
   "Deliberately deferred, not started."** That was accurate when the page
   was written (T-266, before T-195 existed in the codebase) but was stale
   by the time of this audit: `apps/desktop/src/components/BlamePanel.vue`
   and `ReferencesPanel.vue` now exist (commit `3c485be`, "Implement T-195
   (US-062)"), and the task is `review` in Takumi. Corrected in that
   document by this task (see §5).

### 3.2 Must accepted — summary by milestone

| Milestone | Must stories (backlog §4) | Pending in Takumi `todo` | Accepted (`closed` or `review`) |
|---|---:|---:|---:|
| v0.1 | 31 | 0 | 31 |
| v0.2 | 28 | 0 | 28 |
| v0.3 | 19 | 0 | 19 |
| v0.4 | 18 | 0 | 18 |
| v0.5 | 20 | 0 | 20 |
| v1.0 | 12 | 0* | 12* |
| **Total** | **128** | **0** | **128** |

\* One of v1.0's 12 Must stories is T-261/US-128 itself — the task that
produces this document. It is counted as "accepted" only because this
document is its own delivery evidence, not by silently marking it done
elsewhere.

The 12 v1.0-targeted Must stories, individually verified against Takumi
(not inferred from the milestone total alone): US-039/T-172 (`closed`),
US-100/T-215 (`closed`), US-101/T-243 (`closed`), US-102/T-244 (`closed`),
US-103/T-245 (`closed`), US-109/T-251 (`closed`), US-114/T-225 (`closed`),
US-118/T-229 (`closed`), US-123/T-256 (`review`), US-127/T-260 (`review`),
US-128/T-261 (this document), US-133/T-266 (`review`). US-128's own
dependencies — US-127, US-126, US-123, US-118, US-114, US-103, US-109 —
were each individually confirmed `review` or `closed` before writing this
document.

### 3.3 Must in `review` (accepted per this task's own criterion)

Per this task's own instructions, `review` counts as "implemented" for
this checklist — it is not silently promoted to `closed`/"Done" here, and
its real Takumi status is stated plainly so it remains checkable directly
on the board: T-195/US-062, T-254/US-121, T-255/US-122, T-256/US-123,
T-257/US-124, T-258/US-125, T-259/US-126, T-260/US-127, T-264/US-131,
T-265/US-132, T-266/US-133 (plus T-261/US-128 itself once this task is
moved to `review`).

## 4. Should/Could exceptions registered (do not block v1.0)

Only two genuinely unimplemented Should/Could stories remain in the entire
backlog (the other three MoSCoW exceptions in the backlog's distribution
table — US-063, US-117, and the now-corrected US-030 — are implemented and
`closed`):

| ID / Task | Title | MoSCoW | Milestone | Takumi status | Justification |
|---|---|---|---|---|---|
| US-014 / T-147 | Stage by line | Could | v0.3 | `todo` | Self-conditioned by its own Definition of Done on "approved patch-application regressions" reaching review/production. The prerequisite hunk-application tests (US-013/EPIC-06) exist but have had zero time in review, so the story's own stated criterion is not yet met — correctly left in `todo`, not hidden. Also needs a domain-model change (`FileDiff`/`DiffLine` has no field for "no trailing newline" or CRLF) before its criterion 2 can be met. |
| US-104 / T-246 | Create PR/MR | Could | v1.0 | `todo` | Explicitly conditioned in the backlog on "authentication and UX being mature enough" — that condition has not been reassessed this session and no code exists for it. Only read-only PR/MR **listing** (US-103) is implemented; no interface can create, or offer to create, a pull/merge request. |

Neither exception blocks the v1.0 gate: both are Could/Should priority,
both have an explicit, story-level justification recorded in their own
Takumi task and in `docs/manual/roadmap-and-open-decisions.md`, and neither
was converted to `done`/`closed` to make the board look cleaner than it
is.

## 5. Corrective actions taken while delivering this task

- Appended a correction note to Takumi T-162 (US-029) and T-163 (US-030)
  fixing the stale "explicitly deferred, not started" claim now that both
  are implemented (§3.1).
- Updated `docs/manual/roadmap-and-open-decisions.md`: removed the stale
  T-195/US-062 entry from "Deliberately deferred, not started" and updated
  its EPIC-25 section's note that T-261 "remains on the backlog (`todo`)."
- Updated `docs/architecture/release-process.md`'s "Known gaps" bullet
  about T-261 to point at this document instead of describing it as
  outstanding.
- T-261/US-128 itself is moved to Takumi `review` once this document is in
  place, consistent with this session's convention for every other
  delivered story — its dependencies (§3.2) were verified first, not
  assumed.

## 6. Known limitations at this gate (linked, not duplicated)

- No `vX.Y.Z` tag has been pushed; every "version" in §1 is the pre-1.0
  pinned identifier, not a release (`release-process.md`).
- No code signing/notarization; no Marketplace/Open VSX publish — both
  registered decisions blocked on external resources (a certificate, a
  publisher account) this project does not control (ADR-023,
  `release-process.md`).
- Stash/tag/worktree *mutation* UI, mid-flight cancellation of a running
  mutation, Desktop's `--repo`/`--commit` CLI arguments, a Desktop UI to
  connect/disconnect a forge account, and `AbortSignal` plumbing for VS
  Code's blame/history calls are all pre-existing, already-escalated
  product-scope decisions — not silently missed. See
  `docs/manual/roadmap-and-open-decisions.md` and
  `docs/architecture/integrated-regression-report-us123.md`'s "Escalated,
  out-of-scope items (not blocking)" section for the full list and
  rationale of each.
- macOS Intel (`x86_64-apple-darwin`) CLI/TUI archives are not built;
  Desktop's Windows/macOS/AppImage bundles are unverified end-to-end
  outside CI (`release-process.md`'s "Known gaps").
- Trademark/brand-rights availability for the name "GitSail" and its
  assets is not verified (`docs/product/brand-identity.md`).
- Whether submodules are in scope for v1.0 or explicitly post-v1.0 remains
  an open decision (`roadmap-and-open-decisions.md`).

## 7. Filling this matrix at the real release

When a maintainer actually cuts v1.0 (or any `vX.Y.Z` tag):

1. Push the tag; let `.github/workflows/release.yml` run and publish the
   GitHub Release with its `SHA256SUMS.txt`.
2. Replace each `0.0.0`-pinned cell in §1 with the real tag, and link the
   published release and its checksums file.
3. Replace "Platforms built & tested" with what was actually verified for
   *that* artifact set (this document's Desktop AppImage/Windows/macOS
   caveats exist precisely because they were not verified in the sandbox
   this pipeline was authored in — do not carry that caveat forward
   unchanged if a real runner has since verified them).
4. If `SCHEMA_VERSION` changed since this writing, update the
   `schemaVersion` column here and in `protocol-compatibility.md` in the
   same change.
5. Re-run the Takumi `todo`/`review` sweep in §3 before declaring the gate
   met again for any future major release — do not assume it still holds.
