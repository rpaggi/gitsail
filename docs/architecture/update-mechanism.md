# Desktop update checking (T-260/US-127, EPIC-25 — Distribution & Updates)

Companion to ADR-024 (`GitSail_SAD_and_ADRs_v0.1.md`) and to
`docs/architecture/release-process.md` (ADR-023, T-257–T-259). This
document states, concretely, what Desktop's update-check mechanism does,
how it stays consistent with ADR-023's "no code signing yet" decision, and
exactly what is deliberately out of scope — not a silent gap.

## What this is, and what it deliberately is not

**This is a version/origin/integrity *check*, not an updater.** Desktop
asks GitHub's Releases API whether a newer `vX.Y.Z` tag than the one it was
published under exists, and — if so — shows the version, a link to the
GitHub Release page, and a link to that release's `SHA256SUMS.txt`.
Downloading and installing the new version is always a manual action the
person takes themselves, exactly as if they had checked the releases page
by hand.

This is deliberate, not a missing feature: ADR-023 already decided GitSail
distributes via GitHub Releases only, with no code signing/notarization and
no signed auto-update channel. An update mechanism that silently downloaded
and ran an unsigned binary on a person's machine would be a real security
risk (nothing verifies the download came from this project rather than a
compromised mirror or a MITM'd response) and would directly contradict that
decision. The honest, safe version of "update with control" for this stage
of the project is: tell the person a newer version exists, show them
exactly where it came from and how to check it, and let them do the rest
themselves — the same thing this pipeline's own `SHA256SUMS.txt` (T-257,
`release-process.md`) already exists to support.

## Where the pieces live (Ports & Adapters)

| Layer | Type | File |
| --- | --- | --- |
| Domain-agnostic port + use case | `UpdateCheckPort`, `CheckForUpdate`, `ReleaseInfo`, `UpdateCheckOutcome`, `SkipReason` | `crates/gitsail-application/src/update_check.rs` |
| Preferences (the on/off toggle + throttle timestamp) | `Preferences::check_for_updates`/`last_update_check_unix`, `SetCheckForUpdatesPreference` | `crates/gitsail-application/src/preferences.rs` |
| GitHub adapter | `GitHubReleaseUpdateAdapter` (production), `FakeUpdateCheckPort` (test double) | `crates/gitsail-forge/src/release_update.rs` |
| Wire DTOs | `ReleaseInfoDto`, `SkipReasonDto`, `UpdateCheckOutcomeDto`, `PreferencesDto.checkForUpdates` | `crates/gitsail-protocol/src/dto.rs` |
| Desktop's own version identity | `version::RUNNING_VERSION_TAG`/`running_version_tag()` | `apps/desktop/src-tauri/src/version.rs`, `build.rs` |
| Tauri commands | `check_for_update`, `set_check_for_updates`, `open_update_link` | `apps/desktop/src-tauri/src/commands.rs` |
| Frontend store/component | `useUpdateStore`, `UpdateChecker.vue` | `apps/desktop/src/stores/update.ts`, `apps/desktop/src/components/UpdateChecker.vue` |

`gitsail-forge`'s adapter reuses the exact `HttpClient`/`FakeHttpClient`
seam T-245 (`crates/gitsail-forge/src/http.rs`) already established for
GitHub/GitLab PR queries — no second HTTP client was introduced. The GET is
unauthenticated (a public repository's public releases feed needs no
token) and never sends a forge credential.

## Which version am I running? (the ADR-021 gap this story closes)

ADR-021 pins every workspace crate's Cargo version — and
`apps/desktop/src-tauri/tauri.conf.json`'s own `version` field — at
`"0.0.0"` through every pre-1.0 milestone. Neither file can ever tell a
running Desktop process which release it is; the real version identifier
is the git tag `.github/workflows/release.yml` publishes under, exactly
like `gitsail_protocol::SCHEMA_VERSION` is already tracked independently of
any crate's Cargo version (`protocol-compatibility.md`).

**The fix**: `apps/desktop/src-tauri/build.rs` reads the `RELEASE_TAG`
environment variable at compile time. That variable already exists —
`release.yml` sets it at the *workflow* level (`env: RELEASE_TAG:
${{ github.ref_name }}`), which GitHub Actions exposes to every job and
step in the workflow automatically, including the `build-desktop` job's
`npx tauri build` step (which is what actually invokes this crate's `cargo
build`, hence its `build.rs`) — no new wiring was needed in `release.yml`
itself; `build-desktop` was already using this same variable to stamp the
installer's own *display* version (`TAURI_VERSION_OVERRIDE`, T-258).
`build.rs` emits it as a compile-time constant via `cargo:rustc-env
GITSAIL_APP_VERSION=<tag or "dev">`, always defined either way (falls back
to the literal `"dev"` when `RELEASE_TAG` is absent, e.g. `cargo build`/
`cargo test`/`cargo tauri dev` outside the release pipeline).

`version::running_version_tag()` reads that constant with `env!` and turns
the `"dev"` sentinel into an honest `None` — "this build's own version
cannot be determined" — rather than fabricating a `v0.0.0` that could be
misread as a real release. This is a **build-time** constant, deliberately
not a runtime file read or a network call: the version a binary reports
about itself must be exactly the version it was compiled as, and must
never be able to drift after the fact.

## The check flow

1. On Desktop launch, `UpdateChecker.vue`'s `onMounted` hook calls
   `useUpdateStore().check("automatic")`.
2. The `check_for_update` Tauri command calls
   `CheckForUpdate::execute(trigger, running_version_tag(), now)`.
3. For an `"automatic"` trigger, `CheckForUpdate` first loads
   `Preferences`: if `check_for_updates` is `false`, or if
   `last_update_check_unix` is set and less than 24 hours
   (`update_check::ONE_DAY_SECONDS`) have passed, it returns
   `UpdateCheckOutcome::Skipped(..)` **without ever calling the network** —
   this is the mandatory "no unsolicited network call" convention: at most
   once per day automatically, always overridable via the preference.
4. A `"manual"` trigger (an explicit "Check for updates" click) skips both
   gates — an explicit user action is itself the required "form of user
   control."
5. Either way, a real attempt persists `now` as `last_update_check_unix`
   *before* calling the port, so the throttle window advances even for a
   check that then fails — this is what stops "check for updates" +
   "instantly retriggered by the next automatic check" from ever compounding.
6. `GitHubReleaseUpdateAdapter::latest_release()` calls `GET
   https://api.github.com/repos/rpaggi/gitsail/releases/latest`. `200` is
   parsed into a `ReleaseInfo`; `404` means no releases exist yet
   (`NoReleasesPublished`); anything else (5xx, a malformed body, a
   transport failure) becomes `UpdateCheckError::Malformed`/
   `NetworkFailure` — **never a panic**.
7. `CheckForUpdate` compares tags (`vMAJOR.MINOR.PATCH`, parsed strictly —
   an unparseable tag on either side never becomes a silent "equal" or
   "older") and returns one of: `UpToDate`, `UpdateAvailable`,
   `CannotDetermineCurrentVersion` (the running build's own tag is unknown,
   e.g. a local/dev build — the release is still shown, honestly, without
   claiming a comparison that cannot be made), `NoReleasesPublished`, or
   `CheckFailed { diagnostic }`.
8. The frontend renders exactly one of these — see
   `UpdateChecker.vue` — never collapsing "could not check" into "you're up
   to date" or vice versa.

## Failure and recovery (DoD: "falha ou interrupção nunca deixa a instalação inutilizável")

Since this mechanism never replaces or reinstalls anything, "recovery" is
simple by construction:

- A network failure/timeout, or a malformed API response, becomes
  `UpdateCheckOutcome::CheckFailed` — shown as "could not check for
  updates", never a crash, never a blocked UI. The "Check for updates"
  button is immediately usable again; nothing needs to be reset or
  reinstalled.
- `NoReleasesPublished` (GitHub's `404` for "no releases yet") is
  distinguished from a failure — this is a legitimate repository state
  (relevant today: no tag has actually been pushed against this repository
  yet, per `release-process.md`), not an error.
- A malformed/unparseable *latest tag* is treated as a check failure
  (`ErrorCode::ParseFailure`), never silently treated as "up to date" or
  "an update exists" — guessing in either direction would be worse than
  reporting "could not check."

## Protocol compatibility (the honest limitation)

US-127 criterion 3 asks for a protocol-compatibility note before any
version switch. `docs/architecture/protocol-compatibility.md` already
documents that Desktop's own IPC has **no independently-versioned
`schemaVersion` today** — frontend and backend ship as one build from one
`Cargo.lock`, so there is nothing to check between "this build" and
"itself." There is also no way to inspect a *different, not-yet-downloaded*
release's `schemaVersion` without fetching and unpacking its artifacts,
which this check-only mechanism deliberately never does. Rather than
fabricate a per-release compatibility check this mechanism cannot honestly
perform, the update notice does not claim one; the general
protocol-compatibility policy stays documented in
`protocol-compatibility.md`, linked from `troubleshooting.md`, for whoever
needs it when the architecture changes (e.g. if a future daemon/IPC
transport, per ADR-012, makes Desktop's frontend/backend independently
versioned).

## CLI, TUI, and the VS Code extension: no update check (documented, not silent)

None of these three gained any update-check code in this story:

- **CLI/TUI** (`gitsail-cli`, `gitsail-tui`) have no long-lived settings
  surface analogous to Desktop's `AppState`/`PreferencesPort` to hang a
  "check automatically" toggle and its throttle timestamp off of, and no
  natural place to show a persistent notice the way a GUI's sidebar can.
- **The VS Code extension** already has its own, different kind of check —
  `cliLocator.ts`'s `MINIMUM_SUPPORTED_CLI_VERSION` probe (ADR-015) — but
  that verifies the *installed CLI binary* is new enough, it does not ask
  GitHub whether a newer release exists.

Extracting `gitsail_application::update_check::CheckForUpdate` into these
three surfaces later is not architecturally blocked — the use case is
already forge/HTTP-agnostic — but each needs its own place to persist the
throttle state and its own presentation for the result, which this story's
scope did not include. Until that happens, checking
<https://github.com/rpaggi/gitsail/releases> manually is the only way to
know if a newer CLI/TUI/extension build exists — stated in
`docs/manual/troubleshooting.md`'s "Updates" section, not left implicit.

## What is tested

- `crates/gitsail-application/src/update_check.rs`: the throttle/preference
  gate, tag comparison (newer/same/older/unparseable), every
  `UpdateCheckOutcome` variant, and that a manual check both bypasses the
  gate and still advances the throttle timestamp — all against an
  in-memory `PreferencesPort` double and a scripted `UpdateCheckPort`, no
  real network call.
- `crates/gitsail-forge/src/release_update.rs`: GitHub's response-shape
  mapping (200/404/5xx/malformed-body/transport-failure) against
  `FakeHttpClient` — mirrors `github_pr_adapter.rs`'s own test shape.
- `crates/gitsail-protocol/src/dto.rs`: every `UpdateCheckOutcomeDto`
  variant's exact wire shape (camelCase tags/fields), plus
  `PreferencesDto.checkForUpdates`.
- `apps/desktop/src-tauri/src/commands.rs`: `check_for_update`/
  `set_check_for_updates`/`open_update_link` wiring, including the DoD's
  three explicit scenarios — a successful response mapped correctly
  (honestly reported as `CannotDetermineCurrentVersion` in a `cargo test`
  build, since `RELEASE_TAG` is never set there — the "valid update
  reported with the right data" comparison itself is exercised directly
  against `CheckForUpdate` in `gitsail-application`'s own tests, which do
  control the current tag), a malformed response (never panics), and a
  network interruption followed by a successful retry (never left
  unusable) — plus `open_update_link`'s host/scheme re-validation refusing
  a non-`https`/non-`github.com` URL.
- `apps/desktop/src-tauri/src/preferences_store.rs`: `check_for_updates`/
  `last_update_check_unix` round-trip through the JSON file, and a legacy
  file (predating T-260) still defaults `check_for_updates` to `true`.
- `apps/desktop/src/stores/update.test.ts`,
  `apps/desktop/src/services/preferences.test.ts`,
  `apps/desktop/src/stores/preferences.test.ts`: the same three DoD
  scenarios and the preference toggle, against Tauri's own IPC mock
  (`mockIPC`) — no real `invoke` call.

Nothing in this feature's test suite makes a real network call — every
adapter test runs against `FakeHttpClient`/`FakeUpdateCheckPort`/`mockIPC`.

## See also

- ADR-024 (this decision's formal record) and ADR-023 (the GitHub-Releases-
  only distribution decision this extends) —
  `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`.
- `docs/architecture/release-process.md` — the release pipeline this
  mechanism reads from (`RELEASE_TAG`, `SHA256SUMS.txt`).
- `docs/architecture/protocol-compatibility.md` — the compatibility policy
  this document defers to rather than duplicating.
- `docs/manual/desktop.md`'s "Update checking" bullet and
  `docs/manual/troubleshooting.md`'s "Updates" section — the user-facing
  statement of what this does and does not do, and what CLI/TUI/VS Code
  have instead.
