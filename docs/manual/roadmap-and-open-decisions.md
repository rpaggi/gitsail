# Roadmap and open decisions

This page exists to keep two things clearly separated: what GitSail actually
does today (documented in [`tui.md`](./tui.md), [`desktop.md`](./desktop.md),
[`vscode.md`](./vscode.md), and [`troubleshooting.md`](./troubleshooting.md)),
and what is either **deliberately deferred**, **not built yet**, or
**genuinely undecided**. Nothing on this page should be read as "in
progress" or "almost done" — each item states plainly whether work has
started at all.

## Delivered this session (for orientation, not a promise about any other session)

EPIC-16 through EPIC-24 (merge/rebase/conflict recovery, cherry-pick/revert/
reset, stash/tags/worktrees at the Core layer, remote operations, security &
credentials via the OS keyring, preferences consolidation, performance
baseline, CI, and integrated cross-platform testing), most of EPIC-26
(Documentation & Open Source, this task included), and four of EPIC-25's
five stories (T-257/T-258/T-259/T-260 — see below) were implemented and are
in review/pushed. This page is about what was **not** part of that — see
each linked item below for specifics.

T-195/US-062 ("Query blame, tags, stash and remotes" in Desktop, EPIC-12)
was previously listed on this page under "deliberately deferred, not
started" — that has since been implemented (`apps/desktop/src/components/
BlamePanel.vue`, `ReferencesPanel.vue`; commit `3c485be`) and is `review`
in Takumi. T-261/US-128 ("publish the v1.0 release matrix/checklist",
EPIC-25), also previously listed here as remaining on the backlog, is this
same task — see `docs/architecture/v1-release-matrix.md` for the checklist
it produces, including the full Must/Should/Could accounting for v1.0.

## Deliberately deferred, not started

- **T-246 / US-104 — "Create a PR/MR"** (EPIC-20). Explicitly `Could`
  priority in the backlog, conditioned on "authentication and UX being
  mature enough" — that condition has not been reassessed and no code
  exists for it. Only read-only PR/MR **listing** is implemented (Desktop
  only; see `desktop.md`). No interface can create, or offer to create, a
  pull/merge request.
- **T-147 / US-014 — "Stage by line"**. Still on the backlog. Its own
  Definition of Done conditions enabling it on "approved patch-application
  regressions" — the prerequisite hunk-application test coverage
  (US-013/EPIC-06) exists but has had no time in review/production yet, so
  enabling line-level staging now would be premature by the story's own
  stated criterion. The TUI/Desktop only support stage/unstage at the
  file-hunk level today (see `tui.md`/`desktop.md`'s day-to-day operations).
- **Stash/tag/worktree *mutation*, and force-push (with lease), in any UI.**
  Fully implemented and tested at the Core layer (`gitsail-application`/
  `gitsail-git`, EPIC-18/EPIC-19), but no TUI or Desktop action creates/
  applies/pops/drops a stash, creates/deletes/annotates a tag, creates/
  removes a worktree, or force-pushes with a lease. This is a pre-existing,
  already-escalated product-scope decision from earlier epics (see
  `docs/architecture/integrated-regression-report-us123.md`'s
  advanced-operation matrix), not something silently missed.
- **Mid-flight cancellation of a running mutation**, in any interface. The
  only "cancel" semantics anywhere in GitSail is declining a confirmation
  *before* an operation starts; once a mutation is running, it runs to
  completion in every interface today.
- **Desktop's `--repo`/`--commit` command-line arguments** (needed for VS
  Code's "Open Commit in GitSail Desktop" handoff to actually select
  anything). Desktop's Tauri backend does not parse any CLI arguments yet —
  confirmed by reading `apps/desktop/src-tauri/src/{main,lib}.rs`. Tracked
  against EPIC-12/US-056.
- **A UI to connect/disconnect a forge (GitHub/GitLab) account in Desktop.**
  The backend Tauri commands exist and are tested, but no component calls
  them (found while writing this documentation task — see
  `troubleshooting.md`'s credentials section and `desktop.md`'s
  Limitations).
- **`AbortSignal` plumbing for VS Code's blame/history CLI calls.** The
  client already accepts one; the controller never passes one through, so
  a long-running blame/history query cannot be cancelled from the editor.

## EPIC-25 (Distribution & Updates) — partially implemented (T-257/T-258/T-259)

T-257 (distribute the CLI+TUI, US-124), T-258 (package the Desktop app,
US-125), T-259 (publish a compatible VS Code extension package, US-126),
and T-260 (Desktop update checking with integrity/compatibility notes,
US-127, this session) are implemented: `.github/workflows/release.yml`
(ADR-023) builds CLI/TUI archives, Desktop installers, and a VS Code
`.vsix` on every `vX.Y.Z` tag push and publishes them to a GitHub Release
with a `SHA256SUMS.txt`, and Desktop's Settings → "Updates" can check that
same GitHub Release feed and show whether a newer tag exists — see
`docs/architecture/release-process.md` for the release pipeline and
`docs/architecture/update-mechanism.md` for the update-check design. T-261
(publish the v1.0 release matrix/checklist, US-128) is now delivered — see
`docs/architecture/v1-release-matrix.md`. Concretely, as of this writing:

- No `vX.Y.Z` tag has actually been pushed against this repository yet, so
  no GitHub Release exists in practice — the pipeline is ready but has not
  produced a real release. Building from source (see the root `README.md`)
  remains the only way to actually run GitSail today.
- The VS Code extension is not published to the Marketplace or Open VSX —
  a deliberate, registered decision (ADR-023), not a gap. The `.vsix` this
  pipeline builds, installed via "Install from VSIX...", is the official
  path for now. There is nothing else to install alongside it: since
  ADR-025 (supersedes ADR-015) the extension reads Git directly in
  TypeScript rather than shelling out to a `gitsail` binary, so the
  `.vsix` is self-contained and its only requirement is the user's own
  Git ≥ 2.31 on `PATH`.
- **No code signing or notarization** — Windows/macOS installers and
  binaries from this pipeline are unsigned; both OSes will show an
  unrecognized-publisher warning on first run. Recorded explicitly in
  ADR-023 and `docs/architecture/release-process.md`, including exactly
  what would need to change once a certificate/notarization account
  exists — not a silent omission.
- **Desktop can now check for a newer release (T-260/US-127), but this is
  check-only — never an auto-installer.** Desktop's Settings → "Updates"
  compares the running build's own release tag against GitHub's latest
  published release and, when a newer one exists, shows its version, a
  link to the Release page, and a link to its `SHA256SUMS.txt` — download
  and installation stay entirely manual. **CLI, TUI, and the VS Code
  extension still have no update check of any kind** — check
  https://github.com/rpaggi/gitsail/releases yourself for those. Do not
  describe any interface as auto-updating or as verifying a code signature
  — neither exists anywhere in GitSail (ADR-023). See
  `troubleshooting.md`'s "Updates" section and
  `docs/architecture/update-mechanism.md` for exactly what *is* implemented.
- This is expected to remain partial even in a future session for the
  signing/Marketplace pieces specifically, since those (real code-signing
  certificates, marketplace publisher accounts) depend on external
  decisions/resources this project does not control — see
  `docs/architecture/release-process.md`'s own checklist for exactly what
  changes the moment those become available.

## Genuinely open decisions (not yet resolved either way)

These are carried over from the product backlog's own "open decisions and
limits" register (`docs/product/GitSail_Product_Backlog_v1.0.md`), narrowed
to what is still actually unresolved as of this writing — several items
originally on that register have since been resolved by shipped code and
are noted as such:

| Decision | Status |
|---|---|
| License, repository name/organization, pre-1.0 versioning | **Resolved** — Apache License 2.0 (ADR-021), this repository's identity, and versioning are already in effect (see the root `README.md`'s License section). Trademark/brand-rights availability for the name "GitSail" and its assets, however, is explicitly **not verified** — `docs/product/brand-identity.md` keeps the chosen identity without claiming availability was checked. Still open before any public trademark claim. |
| Minimum Git version and Rust MSRV | **Resolved** — Git ≥ 2.31 (ADR-021), Rust MSRV 1.97.0, both pinned and enforced (see `README.md` Requirements). |
| Protocol envelope, cursor, and request correlation | **Resolved for v0.1** — `schemaVersion: 1`, `requestId`, and `nextCursor`/`hasMore` pagination are implemented and covered by compatibility tests (`docs/architecture/protocol-compatibility.md`). |
| Desktop state management and preferences persistence | **Resolved** — Pinia stores plus JSON-file-backed preferences/keybindings/recent-repositories stores exist and are tested (see `troubleshooting.md`'s configuration-files table). |
| Filesystem watcher | **Resolved as "not built, by design"** — manual refresh (on focus and after every mutation) is mandatory and implemented in every interface; an automatic filesystem watcher remains optional and is not implemented. This is a stated scope cut, not a gap. |
| VS Code extension binary distribution | **Resolved — the question no longer has a subject.** ADR-025 (supersedes ADR-015) removed the extension's CLI dependency entirely: it reads Git directly in TypeScript (`apps/vscode/src/git/`), so there is no GitSail binary to bundle, discover, or version-check, and no `gitsail.binaryPath` setting. The `.vsix` is self-contained and needs only the user's own Git ≥ 2.31 on `PATH`; see `vscode.md`. |
| **Submodules: in scope for v1.0, or explicitly post-v1.0?** | **Still open.** No code anywhere in this workspace implements submodule awareness (a single incidental code comment about diff type-changes is the only mention of the word in the whole codebase). The backlog states plainly that no story promises full submodule support and that expanding scope would need a PRD review and new story IDs — that review has not happened. Treat submodules as unsupported until this is explicitly decided. |
| **Exact GitHub/GitLab host/scope boundaries beyond what's built** | **Partially open.** Read-only PR/MR listing is implemented and scoped (title/state/author/branches only); creating a PR/MR is deferred (see T-246 above). Which additional hosts/self-hosted instances are officially supported has not been separately delimited beyond what the existing adapters already handle. |
| **IPC/daemon transport and a plugin policy** | **Open, but explicitly not urgent.** No daemon/IPC transport exists (the protocol's own version-compatibility scaffolding was deliberately built to be ready for one, see `protocol-compatibility.md`), and no plugin system or policy exists. The backlog itself states neither is a dependency for v1.0 — this is future evolution, not a v1.0 gap. |
| **Code-signing/notarization and an update mechanism** | **Signing/notarization: decided (ADR-023) as "not yet, deliberately," blocked on real external resources** (an actual signing certificate/notarization account), not just engineering time — see `docs/architecture/release-process.md` for the exact checklist to reverse this once those resources exist. **Update mechanism (T-260/US-127): resolved for Desktop** — a check-only mechanism against GitHub Releases (no signature verification, consistent with the signing decision above); **CLI/TUI/VS Code still have none.** See `docs/architecture/update-mechanism.md`. Both tracked under EPIC-25 above. |

## Reading this page correctly

- Anything listed above as "not implemented" or "still open" should never be
  presented to a user as available, in progress, or nearly ready — if a
  person asks whether GitSail can do one of these things today, the honest
  answer is no, not yet.
- This page, not the interface manuals, is the place to check before
  promising a capability that "seems like it should already work."
