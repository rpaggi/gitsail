# GitSail troubleshooting and operational guide

Covers what is common to all three interfaces (TUI, Desktop, VS Code):
credentials, where configuration lives, what "updates" means today, privacy,
and protocol/version compatibility. For interface-specific commands, see
[`tui.md`](./tui.md), [`desktop.md`](./desktop.md), and
[`vscode.md`](./vscode.md). For what is planned but not built, and for
decisions still genuinely open, see
[`roadmap-and-open-decisions.md`](./roadmap-and-open-decisions.md) — this
document only covers what exists in the code today.

## Credentials: Git and forge accounts are never GitSail's own vault

**GitSail delegates authentication entirely to existing Git/SSH/OS
credential mechanisms.** It never writes a password or token in plaintext,
and it never builds a credential store of its own beyond OS-native secure
storage (wiki: "Security, Privacy & Credentials Rules", rule 3).

- **`fetch`/`pull`/`push` over SSH or HTTPS** use whatever Git, your SSH
  agent, and your OS's Git credential helper already have configured — the
  exact same credential resolution `git` itself would use from a plain
  terminal. GitSail never prompts for or stores a Git remote password/token
  itself; only `gitsail-git` invokes `git` at all (enforced by
  `scripts/ci/check-architecture.sh`'s second check), and it always just
  shells out with your existing credential setup already in place.
- **Forge (GitHub/GitLab) account tokens** — used only for the read-only
  Pull/Merge Request listing — are stored via the `keyring` crate
  (`crates/gitsail-forge/src/keyring_store.rs`), under one fixed service
  name `"gitsail-forge"`, with a distinct entry per account
  (`github@github.com`, `gitlab@gitlab.example.com`, …). This puts every
  GitSail-managed forge secret in your OS's own credential UI — Keychain
  Access on macOS, Credential Manager on Windows, Seahorse/KWalletManager on
  Linux — never in a GitSail-specific file on disk. No token content ever
  appears in a GitSail error message (every message is passed through a
  redaction function as defense in depth, even though nothing in that code
  path is expected to embed one).
- **Manual verification limitation**: connecting/disconnecting/reading a
  forge token against a *real* OS keyring cannot be exercised in this
  project's own automated test/CI sandbox — only entry construction
  (service/username naming) is unit-tested there. Reading and writing an
  actual OS-native secret must be verified manually, per target OS, before
  relying on it (`keyring_store.rs`'s own module doc records the same
  limitation).
- **No UI to connect a forge account exists yet.** The backend Tauri
  commands (`connect_forge_account`, `disconnect_forge_account`,
  `forge_connection_status`) exist and are tested, but no Desktop component
  calls them (confirmed by reading every file under
  `apps/desktop/src/components` and `src/stores` — see `desktop.md`'s
  Limitations). Practically, today, a repository with a private GitHub/
  GitLab remote will show "no token" in the Pull/Merge Requests panel with
  no in-app way to fix that yet.
- **GitSail never requires a central GitSail account.** Every local Git flow
  works with no forge account connected and no network reachable at all —
  re-verified end to end in `docs/architecture/integrated-regression-report-us123.md`
  (criterion 3: `gitsail-cli` and `gitsail-tui` do not even link the one
  crate capable of making an HTTP call; only `apps/desktop/src-tauri`'s
  single, explicit pull/merge-request listing command ever reaches a forge
  API).

## Configuration files: where each interface keeps its settings

See `docs/architecture/preferences-matrix.md` for the full rationale of what
is shared vs. interface-specific vs. "shared concept, separately stored."
The concrete files, as of this writing:

| Interface | What | Location |
|---|---|---|
| TUI | Keybindings override | `<OS config dir>/gitsail/tui/keybindings.conf` (override with `--keybindings <path>`) — plain text, `action-id = X` per line. No theme/preferences file: the TUI has no light/dark concept of its own (`--ascii`/`NO_COLOR` is a color-vs-text-only rendering switch, not a theme). |
| Desktop | Theme | `<OS config dir>/gitsail/desktop/preferences.json` |
| Desktop | Keyboard shortcut overrides | `<OS config dir>/gitsail/desktop/keybindings.json` |
| Desktop | Recent repositories | `<OS config dir>/gitsail/desktop/recent-repositories.json` |
| VS Code | All `gitsail.*` settings (`gitsail.binaryPath`, `gitsail.blame.*`, `gitsail.desktop.path`) | VS Code's own `settings.json` (Settings UI, or `Ctrl/Cmd+,` → open as JSON) — GitSail contributes configuration keys to VS Code's existing store; it does not keep a separate file. VS Code itself keeps that file under its own per-OS user-data directory. |

`<OS config dir>` is whatever the OS-native config directory convention
resolves to for the account running GitSail (e.g. `~/.config` on Linux,
`~/Library/Application Support` on macOS, `%APPDATA%` on Windows) — every
store above falls back to safe defaults if its file is missing, unreadable,
or corrupted; none of them ever crash on a bad config file.

A destructive-operation confirmation policy can **never** be weakened by any
preference, in any interface — this is enforced structurally (a fixed,
non-configurable check in `gitsail-application`, plus, per interface, code
paths a keybinding remap or config file cannot reach), not merely
documented convention. See `preferences-matrix.md`'s "confirmation-policy
invariant" section for the file:line proof.

## Updates: there is no automatic update mechanism today

**Be direct about this: GitSail does not check for, download, or install
updates automatically, in any interface, as of this writing.** EPIC-25
("Distribution & Updates") — the epic that would define signing,
packaging, and an update mechanism — has not been implemented in this
session. Concretely:

- **CLI/TUI**: no prebuilt binary is distributed at all yet; build from
  source (see the root `README.md`'s "Installation (from source)"). There
  is no version-check or update-download code anywhere in `gitsail-cli`/
  `gitsail-tui`.
- **Desktop**: no packaged installer exists for any OS; it is run from
  source via `npm run tauri dev`. No auto-update mechanism, no signature/
  notarization strategy is implemented.
- **VS Code extension**: not published to any marketplace; it does not
  bundle or fetch a `gitsail` binary on your behalf (a deliberate decision,
  ADR-015 — see `vscode.md`). What *is* implemented is a **verification**
  check, not an update mechanism: before running any query, the extension
  spawns `<binary> --version` and compares it against a minimum supported
  version, so a too-old or unrecognized binary is reported clearly instead
  of silently misbehaving — this checks compatibility, it does not fetch or
  install anything.

If you need a newer GitSail, rebuild/reinstall from source yourself today.
Anything describing an installer, an auto-updater, or a signed release
artifact belongs to EPIC-25 and is **not implemented** — see
[`roadmap-and-open-decisions.md`](./roadmap-and-open-decisions.md).

## Privacy

- **No telemetry, ever, by default.** `crates/gitsail-application/src/privacy.rs`
  defines `TelemetryPreference` and `CrashReportConsent`, both defaulting to
  disabled/not-granted — but as that module's own doc states, **no
  telemetry or crash-report upload implementation exists at all yet**; these
  types are a reserved, explicit configuration surface for if/when such a
  capability is ever built, specifically so its default is opt-in from the
  very first line of code that reads it, never a default silently added
  later. Confirmed by the absence of any HTTP/network client dependency
  anywhere in this workspace except `gitsail-forge`'s narrowly-scoped forge
  API calls (see the credentials section above).
- **No central GitSail account, ever required.** Every local Git flow works
  fully offline with no GitSail account, anywhere.
- **Logs and diagnostics never contain secrets by default**: the TUI's
  `--debug` diagnostics, and any future exported diagnostic bundle, must
  never contain tokens, passwords, credential-bearing URLs, or full file
  content by default (wiki: "Security, Privacy & Credentials Rules", rule
  5) — this is a standing rule for any diagnostics surface added later, not
  only what exists today.
- **Repository and forge content is always treated as untrusted input** —
  never interpolated into a shell, never executed, and (in VS Code)
  Markdown-escaped before ever reaching a hover, with only an explicit,
  scoped set of command links ever enabled (`hoverSanitizer.ts`, see
  `vscode.md`'s module map). A commit message can never become an
  executable link or a shell command by virtue of being displayed.

## Protocol and version compatibility

See `docs/architecture/protocol-compatibility.md` for the full matrix and
its compatibility-test inventory. Summary:

- **`schemaVersion`** (the shape of the JSON envelope `gitsail-cli --json`
  produces) is currently `1`, understood by the VS Code extension's
  `SUPPORTED_SCHEMA_VERSIONS`. The TUI and Desktop never cross this boundary
  at all — they call `gitsail-application`/`gitsail-domain` directly from
  the same compiled binary, so Rust's own type system is the compatibility
  check for them, not a runtime schema negotiation.
- **The `gitsail-cli` binary version** is a separate concern from
  `schemaVersion` — the VS Code extension checks it independently via
  `cliLocator.ts`'s `MINIMUM_SUPPORTED_CLI_VERSION` probe (see "Updates"
  above). A binary can be new enough on that check and still, in principle,
  emit a `schemaVersion` a given extension build does not speak; both
  checks exist and are both required.
- **Minimum Git version**: 2.31 (the floor GitSail's Git adapter actually
  needs, derived from the literal CLI arguments it passes — see ADR-021 and
  the root `README.md`'s Requirements section). **Rust MSRV**: 1.97.0,
  pinned in the workspace `Cargo.toml`.
- **A missing or unrecognized `git`** produces a distinct, explicit
  `ErrorCode::GitNotInstalled`/`UnsupportedGitVersion` in every interface —
  none of them silently degrade to guessing.

## Performance expectations

See `docs/architecture/performance-baseline.md` for the full, reproducible
measurement. In short: on the measured environment, a first-page log/diff/
blame call is dominated by a fixed ~20 ms per-`git`-invocation floor
(process-wait polling), essentially flat from 100 to 5,000 commits — first
page never scans full history, and a cancelled query returns in about that
same ~20 ms rather than in proportion to the cancelled work's size. This is
one reproducible data point with its environment recorded, not a promised
SLA for every machine.

## Diagnosing a problem

1. **A local Git operation fails** — the error message states a specific
   `ErrorCode` and, when available, a remediation hint; none of the three
   interfaces ever silently swallows a Git failure into a generic message.
2. **A remote operation (fetch/pull/push) fails** — GitSail never guesses at
   network/auth failures; a repository with no remote configured reports
   that clearly instead of attempting any network reach, and a rejected
   push is never silently escalated into a force push in any interface.
3. **The TUI/CLI reports `GitNotInstalled` or an unsupported Git version** —
   install/upgrade Git to at least 2.31, or pass `--git-path` to point at a
   specific executable.
4. **The VS Code extension reports the binary is missing/incompatible** —
   install a `gitsail` build meeting `cliLocator.ts`'s minimum version and
   either put it on `PATH` or set `gitsail.binaryPath` (trusted workspaces
   only).
5. **A destructive operation you didn't intend to run** — this should be
   structurally impossible: every `Moderate`/`Destructive` mutation, in
   every interface, always requires an explicit confirmation step first,
   and declining it leaves the repository completely untouched. If you ever
   see otherwise, that is a bug against the guarantees in
   `preferences-matrix.md`'s "confirmation-policy invariant" section, not
   expected behavior.
