# GitSail release process (T-257/T-258/T-259, EPIC-25 — Distribution & Updates)

This document is the operational companion to ADR-023
(`GitSail_SAD_and_ADRs_v0.1.md`). It states, concretely, what
`.github/workflows/release.yml` builds and publishes, the real distribution
decisions behind it, and exactly what is deliberately deferred — not a
silent gap — until the user (the repository owner) has certificates/accounts
this project does not control.

## The decision this document records

The repository owner made an explicit, real product decision this session:
**distribute via GitHub Releases only, for now.** GitSail is used personally
first, then released on GitHub for anyone who wants to try it. Code
signing/notarization and Marketplace/Open VSX publishing are **deliberately
postponed**, not attempted-and-failed — they depend on a code-signing
certificate and marketplace publisher accounts the owner does not yet have.
This is a product/business decision, not a technical limitation of this
pipeline: everything this pipeline can do without those two external
resources, it does.

## What `.github/workflows/release.yml` does

Triggered by pushing a `vX.Y.Z` tag (or manually dispatched against an
existing such tag — a `guard` job refuses to run against anything else, so a
release can never accidentally get published from `main` or a branch name).

| Job | Runs on | Produces |
| --- | --- | --- |
| `guard` | ubuntu-latest | Fails fast unless `github.ref` is `refs/tags/v*`. |
| `build-cli-tui` | ubuntu-latest, windows-latest, macos-latest (matrix) | `cargo build --release -p gitsail-cli -p gitsail-tui`, then one archive per OS (`gitsail-<tag>-linux-x86_64.tar.gz`, `-windows-x86_64.zip`, `-macos-aarch64.tar.gz`) containing both binaries, `LICENSE`, a per-OS Git-prerequisite `README.txt` (`docs/architecture/release-artifact-readme.txt`), and `VERSION.txt`. |
| `build-desktop` | ubuntu-latest, windows-latest, macos-latest (matrix) | `npx tauri build` (Tauri's own bundler, `apps/desktop/src-tauri/tauri.conf.json`'s `bundle` config) restricted per OS to `deb,appimage` / `msi,nsis` / `dmg,app` — see "Why not `rpm`" below. |
| `build-vscode` | ubuntu-latest only | `npx vsce package` → `gitsail-vscode-<tag>.vsix`, after copying the root `LICENSE` into `apps/vscode/` so the packaged extension actually embeds the license text (see "VS Code metadata" below) and stamping `package.json`'s `version` to the release tag for this build only. |
| `publish-release` | ubuntu-latest | Downloads every job's artifacts, computes `SHA256SUMS.txt` over all of them, and runs `gh release create <tag> --notes-file docs/architecture/release-notes-template.md <all artifacts>` using the default `GITHUB_TOKEN` — no extra secrets. |

### Why not `rpm`

Tauri's Linux bundler can also produce a `.rpm`, and `tauri.conf.json`'s
`bundle.targets` stays `"all"` so a person building locally still gets
whatever their machine supports. The **release pipeline** explicitly passes
`--bundles deb,appimage` on Linux instead of `all`, for two reasons: (1)
US-125's own acceptance criterion only names `.deb`/`.AppImage` for Linux —
`.rpm` was never asked for; (2) `rpmbuild` is not preinstalled on GitHub's
`ubuntu-latest` runner image (unlike `dpkg-deb`, which is), so requesting it
without first adding an install step would either fail the whole `deb+rpm+
appimage` bundle step or silently need extra CI maintenance for a format
nobody asked for. Adding `.rpm` later is a small, isolated change (install
`rpm`, add `rpm` to the `--bundles` list) if a Fedora/openSUSE-focused user
ever asks for it.

### Artifact versioning (US-124 criterion 2 / US-125 criterion 2)

Per ADR-021 (pre-1.0 versioning policy), every workspace crate's Cargo
version — and `apps/desktop/src-tauri`'s `Cargo.toml`/`tauri.conf.json`
`version` field, and `apps/vscode/package.json`'s `version` field as
committed — stays `"0.0.0"` through every pre-1.0 milestone. The release
pipeline does **not** change that in the repository; it overrides the
*build-time* version only, for that one CI run, via:
- `cargo build`'s own binaries: unaffected — `gitsail --version` /
  `gitsail-tui --version` intentionally still print `0.0.0`. The release
  tag (in the archive's filename and its bundled `VERSION.txt`) is the
  authoritative version identifier for a release, not the Cargo version —
  exactly like ADR-016's `SCHEMA_VERSION` is already tracked independently
  of any crate's Cargo version.
- Tauri's bundle metadata: `npx tauri build --config '{"version":"<tag
  without the leading v>"}'` (CLI-side JSON merge, no file edited) — so the
  `.msi`/`.deb`/`.dmg` a person downloads shows the real version in their
  OS's package manager/Programs list, not a confusing `0.0.0`.
- The `.vsix`: `npm version <tag without v> --no-git-tag-version
  --allow-same-version` inside the `build-vscode` job's own checkout —
  again never committed back to the repository, just stamped into that
  job's throwaway `package.json` before `vsce package` reads it.

### Checksums and verifiable origin (US-124 criterion 2)

`publish-release` runs `sha256sum *` over every artifact from every other
job and attaches the resulting `SHA256SUMS.txt` to the GitHub Release
alongside them. "Origin" is the GitHub Release itself — built by this
workflow, from this exact tagged commit, with the workflow run's own log as
the build provenance. This is **not** code signing (see below) — it lets
someone confirm a downloaded file matches exactly what this pipeline
produced, not that a particular organization vouches for it cryptographically.

## Registered decision: no code signing or notarization yet

**Decision (ADR-023):** GitSail's Windows installers are not Authenticode
code-signed, and macOS installers are not signed/notarized, in this initial
distribution pipeline. This means:
- **Windows**: SmartScreen will show an "unrecognized publisher" warning the
  first time someone runs the `.msi`/NSIS `.exe`. The user must click
  "More info" → "Run anyway" (or an equivalent OS-specific override).
- **macOS**: Gatekeeper will refuse to open the unsigned `.app`/`.dmg` by
  default ("cannot be opened because the developer cannot be verified"). The
  user must explicitly allow it (System Settings → Privacy & Security →
  "Open Anyway", or `xattr -d com.apple.quarantine <path>` for the
  technically inclined).
- **Linux**: `.deb`/`.AppImage` are unaffected — Linux has no equivalent
  OS-level gate for unsigned binaries by default.

This is recorded here, in `release-notes-template.md` (so every release's
own notes restate it), and in each CLI/TUI archive's `README.txt`, so nobody
installing GitSail is surprised by a warning nothing told them to expect.

**What would need to change once a certificate/account exists** (this is
the concrete "débito registrado para o futuro" checklist, not a vague
someday):
1. **Windows Authenticode**: obtain a code-signing certificate (from a CA,
   or an EV cert for immediate SmartScreen reputation). Add a signing step
   to `build-desktop`'s Windows leg *before* `npx tauri build` finishes
   bundling — Tauri v2's `bundle.windows.certificateThumbprint` (plus
   `digestAlgorithm`, `timestampUrl`) in `tauri.conf.json`, or an explicit
   `bundle.windows.signCommand` if using a cloud HSM/signing service. The
   certificate/private key would need to be stored as a GitHub Actions
   secret (this pipeline currently uses **zero** secrets beyond the default
   `GITHUB_TOKEN` — that changes the moment this step is added).
2. **macOS signing + notarization**: enroll in the Apple Developer Program,
   generate a Developer ID Application certificate, and set
   `bundle.macOS.signingIdentity` in `tauri.conf.json` (plus
   `bundle.macOS.hardenedRuntime`/entitlements as needed). Notarization
   itself is a separate post-build step (`xcrun notarytool submit` +
   `stapler`) that would need to run after `tauri build` produces the
   `.app`/`.dmg`, using an app-specific password or API key stored as a
   GitHub Actions secret.
3. **CLI/TUI binaries**: today they ship entirely unsigned on every OS.
   Once a Windows certificate exists, the same Authenticode signing step
   should also sign `gitsail.exe`/`gitsail-tui.exe` before they go into the
   `.zip`; once a macOS Developer ID exists, the same applies to the raw
   `gitsail`/`gitsail-tui` macOS binaries before they go into the
   `.tar.gz` (Gatekeeper's "unidentified developer" gate applies to any
   downloaded executable, not just `.app` bundles).
4. Once any of the above exists, remove the corresponding warning from
   `release-notes-template.md` and each archive's `README.txt` — do not
   leave stale "this is unsigned" language once it no longer is.

Nothing above is implemented by this change — it is the exact list of what
changes, and where, the next time this decision is revisited.

## Registered decision: VS Code Marketplace / Open VSX stay unpublished

**Decision (ADR-023, originally extending ADR-015 — see ADR-025, which
superseded ADR-015 and made the `.vsix` self-contained):** the `.vsix` built by this
pipeline is attached to the GitHub Release and is the **official
installation path today** — "Extensions" view → "..." menu → "Install from
VSIX..." → pick the downloaded file (or `code --install-extension
gitsail-vscode-<tag>.vsix` from a terminal). Publishing to the VS Code
Marketplace (`vsce publish`) or Open VSX (`ovsx publish`) is deliberately
not done, because both require a registered publisher account/token this
project does not yet have:
- **VS Code Marketplace** needs a Microsoft/Azure DevOps organization and a
  Personal Access Token tied to the `publisher` id in `package.json`
  (currently `"gitsail"` — **this id has never been registered on the
  Marketplace; treat it as a placeholder to confirm or change before any
  real `vsce publish`**, not a reserved, confirmed identity).
- **Open VSX** needs a separate namespace registration and access token
  (`ovsx create-namespace`/`ovsx publish`), independent of the Marketplace
  one.

**What would need to change once those accounts exist:**
1. Register the `gitsail` publisher id on the Marketplace (or pick and
   record a different one, updating `apps/vscode/package.json`'s
   `publisher` field to match — never assume `gitsail` is available without
   checking).
2. Add a `VSCE_PAT`/`OVSX_PAT` GitHub Actions secret.
3. Add a `publish-vscode-marketplace` job to `release.yml` (or a manually
   triggered follow-up workflow, to keep an accidental re-publish from ever
   happening on every tag automatically) that runs `npx vsce publish
   --pat "$VSCE_PAT"` / `npx ovsx publish gitsail-vscode-<tag>.vsix -p
   "$OVSX_PAT"` against the same `.vsix` this pipeline already builds and
   checksums — the packaging step does not change, only a new publish step
   is added after it.
4. Update this document and `apps/vscode/README.md`'s installation
   instructions once that first real publish happens — "Install from VSIX"
   would then become a fallback, not the primary path.

### VS Code `package.json` metadata (US-126 criterion 3)

Confirmed present before this change: `publisher` (`"gitsail"` — see the
placeholder caveat above), `license`, `engines.vscode`, `categories`,
`activationEvents`, `contributes.configuration`/`commands`/`menus`, and the
untrusted-workspace `capabilities` declaration (ADR-010-adjacent: GitSail
context is unavailable until the workspace is trusted). Added by this
change: `repository`, `bugs`, `homepage` (all pointing at
`github.com/rpaggi/gitsail`, matching ADR-021's repository-identity
decision) — these were missing and are conventional/expected metadata for
any VS Code extension, Marketplace-published or not. `@vscode/vsce` was
added as a `devDependency` (it did not exist before), plus a
`vscode:prepublish` script (`npm run compile`, so `vsce package` always
packages freshly compiled `out/*.js`, never stale ones) and a `package`
script (`vsce package`, for a local/manual build).

## Desktop packaging (T-258/US-125)

### Formats and dependencies (criterion 1)

`apps/desktop/src-tauri/tauri.conf.json`'s `bundle.targets` was already
`"all"` before this change (confirmed by reading the file) — Tauri v2
already knew how to produce every format US-125 asks for; nothing about
*which* formats needed configuring. What this change adds to that same
`bundle` block is metadata Tauri's bundler feeds into each package's own
manifest: `publisher`, `homepage`, `copyright`, `license` (`"Apache-2.0"`,
matching ADR-021), `category`, `shortDescription`, `longDescription`.
Verified locally (see "What was verified" below): building a real `.deb`
with this exact config produces a control file with
`Depends: libwebkit2gtk-4.1-0, libgtk-3-0` — Tauri's bundler already derives
the correct runtime dependency list for Linux automatically; nothing needed
adding by hand. `README.md`'s Requirements section and `ci-policy.md`
already document the *build-time* Linux dependency list
(`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, etc.) — the *runtime* list a
`.deb` install pulls in is a strict subset of that (dev packages install
their runtime counterparts as a dependency, e.g. `libgtk-3-dev` depends on
`libgtk-3-0`), consistent with what the built `.deb` actually declared.

### Signing/notarization (criterion 2)

See "Registered decision: no code signing or notarization yet" above — this
is the same pipeline-wide decision, not a Desktop-specific one, since
Tauri's Windows/macOS bundlers are the same tool this criterion is about.

### Install/remove preserves repositories and preferences (criterion 3)

Verified by reading the actual persistence code, not assumed:
`apps/desktop/src-tauri/src/preferences_store.rs` and
`recent_repositories_store.rs` both resolve their file path via the `dirs`
crate to `<OS config dir>/gitsail/desktop/{preferences,recent-repositories}.json`
— e.g. `~/.config/gitsail/desktop/` on Linux, `~/Library/Application
Support/gitsail/desktop/` on macOS, `%APPDATA%\gitsail\desktop\` on Windows
(see `docs/architecture/preferences-matrix.md`, T-247–T-249). This
directory is **entirely separate from where any installer places the
application binary itself** (`/opt`/`/usr` + `.desktop` file for a `.deb`,
`Program Files` for the NSIS/MSI installer, `/Applications` for the `.app`):
- A `.deb`/`.msi`/`.dmg` **uninstall** removes the installed application
  files only. None of the three installer formats GitSail uses touches the
  OS config directory — Tauri's generated uninstallers do not add any such
  step, and nothing in this codebase writes GitSail's own data anywhere
  under the app's install directory.
- The user's actual Git repositories are untouched by definition: GitSail
  never installs into, or stores its own data inside, any repository it
  reads — install/open/close/remove of the *application* has no code path
  that touches a repository the user pointed GitSail at.
- Clearing preferences/recent-repositories is only possible by the user
  manually deleting that OS config directory (or a future in-app "reset"
  action, which does not exist today) — never as a side effect of
  installing, opening, or uninstalling the app.

This confirms `preferences-matrix.md`'s existing documentation was already
accurate; nothing needed correcting, only cross-referencing here for this
story's own criterion.

## What was verified locally (sandbox limits, same discipline as `ci-policy.md`)

This pipeline could not be executed against real GitHub Actions from the
environment this task was implemented in (no GitHub Actions access, no
Windows/macOS runners — the same limitation `ci-policy.md` already records
for `ci.yml`). What *was* run directly, on Linux, before this document
existed:
- `cargo build --release -p gitsail-cli -p gitsail-tui` — succeeds; verified
  `gitsail`/`gitsail-tui` are the actual produced binary names (no
  `[[bin]]` override in either crate's `Cargo.toml`) and that
  `--version` prints `0.0.0` per ADR-021, confirming the versioning section
  above.
- `npx tauri build --bundles deb` from `apps/desktop`, with the exact
  `tauri.conf.json` metadata this change adds — succeeds, and
  `dpkg-deb -I` on the resulting `.deb` confirms `Homepage`, `Maintainer`,
  `Description` and the auto-derived `Depends` line all render correctly.
  `--bundles appimage` **was** attempted (network access to download
  `linuxdeploy`/`AppRun` turned out to be available in this sandbox) but
  failed with a sandbox-specific error, not a workflow bug: `linuxdeploy`
  crashed (`boost::filesystem::filesystem_error: Permission denied` on
  `/mnt/c/Windows/system32/config/systemprofile/...`) while scanning `PATH`
  — this development environment is WSL2 (per its own environment banner),
  which injects the Windows host's `PATH` entries (`/mnt/c/Windows/...`)
  into the Linux `PATH`, and `linuxdeploy` does not handle a permission
  error on one of those entries gracefully. A real `ubuntu-latest` GitHub
  Actions runner has no such WSL/Windows-path interop, so this specific
  failure is not expected there — but it was not possible to *prove* that
  from this sandbox, only to identify the cause precisely enough to rule
  out a `release.yml` defect. `msi`/`nsis`/`dmg`/`app` were not attempted
  at all — they need a Windows/macOS host, neither available here. This
  mirrors `ci-policy.md`'s own precedent for `ci.yml`'s Windows/macOS legs.
- `apps/vscode`: `npm install` (to actually add `@vscode/vsce`, not just
  edit `package.json` by hand), then `npx vsce package` — succeeds, and was
  run twice: once without copying `LICENSE` into `apps/vscode/` first
  (confirmed the resulting `.vsix` contains no license file, only a
  build-log warning) and once with the copy (confirmed the `.vsix`'s file
  listing includes `LICENSE`) — this is exactly why `build-vscode`'s job
  includes that copy step rather than leaving `vsce`'s warning unaddressed.
- `.github/workflows/release.yml`'s syntax — validated two ways, same as
  `ci-policy.md`'s own precedent for `ci.yml`: `python3 -c "import yaml;
  yaml.safe_load(open(...))"` (parses cleanly), and `actionlint` (installed
  via `go install github.com/rhysd/actionlint/cmd/actionlint@latest`,
  `go`/network were both available this session) — run against both
  `release.yml` and the pre-existing `ci.yml` together, zero findings on
  either.
- `gh release create` itself was **not** run — it requires an authenticated
  `GITHUB_TOKEN` against the real repository and would actually publish a
  release; nothing in this sandbox can safely exercise that.

## Known gaps

- **macOS Intel (x86_64-apple-darwin) CLI/TUI binaries are not built.**
  `build-cli-tui`'s macOS leg only produces an `aarch64-apple-darwin`
  archive, matching GitHub's `macos-latest` runner's native architecture.
  Adding an Intel build is a small, well-understood addition (`rustup
  target add x86_64-apple-darwin` + a second `cargo build --release
  --target x86_64-apple-darwin -p gitsail-cli -p gitsail-tui` on the same
  runner — Xcode's toolchain cross-compiles both Apple architectures
  without extra linkers) but was not added here to keep this first version
  of the pipeline to what could be reasoned about and at least partially
  verified in this sandbox; deferred, not forgotten.
- **Desktop's AppImage/macOS/Windows bundles are unverified end-to-end.**
  `.deb` was built and inspected successfully; `--bundles appimage` was
  attempted but hit a WSL-specific sandbox failure (see above) before ever
  reaching Tauri's own bundling logic, so it proves nothing either way
  about whether `--bundles deb,appimage` genuinely succeeds on a real
  `ubuntu-latest` runner. `msi`/`nsis`/`dmg`/`app` were not attempted at
  all — no Windows/macOS host was available in this sandbox (same
  limitation as `ci.yml`'s own Windows/macOS legs, per `ci-policy.md`).
- **`.rpm` is not part of the release pipeline** — see "Why not `rpm`"
  above; a deliberate scope cut, not an oversight.
- **v1.0 release matrix/checklist** (T-261/US-128) is now delivered — see
  `docs/architecture/v1-release-matrix.md`, which also lists this
  document's own known gaps (macOS Intel, unverified AppImage/Windows/
  macOS bundles) as part of its component/platform matrix. (T-260/US-127's
  Desktop update-check mechanism, previously listed here as unimplemented,
  now exists — see `docs/architecture/update-mechanism.md` and ADR-024.
  CLI, TUI, and the VS Code extension still have no update check of any
  kind.)

## See also

- `docs/architecture/update-mechanism.md` (T-260/US-127, ADR-024) — the
  Desktop update-*check* mechanism built on top of this pipeline's
  artifacts/checksums; explicitly not an auto-updater, consistent with
  this document's own no-signing decision.
- ADR-023 (this decision's formal record) and ADR-015 (VS Code binary
  distribution, which this document extends) —
  `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md`.
- `docs/architecture/ci-policy.md` — the pre-existing `ci.yml` pipeline this
  workflow's conventions were deliberately kept consistent with.
- `docs/architecture/preferences-matrix.md` — the preferences/persistence
  design this document's "Install/remove preserves..." section relies on.
- `docs/manual/roadmap-and-open-decisions.md` — the up-to-date, user-facing
  statement of what EPIC-25 has and has not shipped.
