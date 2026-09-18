# GitSail — Software Architecture Document
**Architecture baseline:** v0.1  
**Covers roadmap:** v0.1 → v1.0  
**Status:** Initial architecture  
**Product:** GitSail  
**Tagline:** *Navigate your Git history.*

## ADR Index

This document holds both the Software Architecture Document (§1–§38) and every Architecture Decision Record (after §38), in one file, by original design. This index exists so all 24 ADRs are accessible with their status from the top of the document, without scrolling past the SAD body first (T-264/US-131 criterion 2). All 24 are `Accepted`; none has been superseded or rejected as of this writing.

| ADR | Title | Status | Summary |
| --- | --- | --- | --- |
| [ADR-001](#adr-001--rust-for-the-shared-core) | Rust for the shared core | Accepted | Use Rust for domain, application, Git infrastructure, protocol support, CLI and TUI. |
| [ADR-002](#adr-002--ports--adapters-architecture) | Ports & Adapters architecture | Accepted | Domain, Application, Ports, Infrastructure Adapters and Presentation as separate layers. |
| [ADR-003](#adr-003--git-cli-as-initial-git-provider) | Git CLI as initial Git provider | Accepted | Use the installed `git` executable through a dedicated adapter, not a native Git library. |
| [ADR-004](#adr-004--monorepo) | Monorepo | Accepted | Keep GitSail components in one repository initially. |
| [ADR-005](#adr-005--ratatui-for-tui) | Ratatui for TUI | Accepted | Use Ratatui for terminal rendering and interaction. |
| [ADR-006](#adr-006--tauri--vue-3-for-desktop) | Tauri + Vue 3 for Desktop | Accepted | Tauri as desktop shell/backend integration, Vue 3 for the frontend. |
| [ADR-007](#adr-007--typescript-for-vs-code-extension) | TypeScript for VS Code extension | Accepted | Implement the extension in TypeScript, consuming GitSail through a structured external boundary. |
| [ADR-008](#adr-008--versioned-structured-protocol) | Versioned structured protocol | Accepted | Explicit versioned DTOs in `gitsail-protocol`; JSON as the initial serialization format. |
| [ADR-009](#adr-009--separate-read-and-mutation-capabilities) | Separate read and mutation capabilities | Accepted | Separate read-oriented and mutation-oriented ports/use cases where practical. |
| [ADR-010](#adr-010--gitsail-does-not-own-credentials) | GitSail does not own credentials | Accepted | Delegate Git transport credentials to Git/SSH/OS credential helpers; future forge tokens use secure OS storage. |
| [ADR-011](#adr-011--graph-layout-separated-from-rendering) | Graph layout separated from rendering | Accepted | Implement graph semantics/layout independently of UI rendering. |
| [ADR-012](#adr-012--cli-json-before-daemonipc) | CLI JSON before daemon/IPC | Accepted | Use CLI JSON as the first cross-process transport; revisit a local daemon/IPC later. |
| [ADR-013](#adr-013--clap-for-cli-argument-parsing) | clap for CLI argument parsing | Accepted | `clap` (derive API) for `gitsail-cli`; usage errors keep clap's own exit code `2`, distinct from domain-error codes. |
| [ADR-014](#adr-014--protocol-envelope-correlation-and-cursor-shape) | Protocol envelope, correlation and cursor shape | Accepted | `Envelope<T>` tagged by `status`, always carrying `schemaVersion` and `requestId`; opaque `nextCursor`/`hasMore` pagination. |
| [ADR-015](#adr-015--vs-code-binary-distribution-for-v04) | VS Code binary distribution for v0.4 | Accepted | The extension discovers `gitsail`/`gitsail.exe` on `PATH` or an explicit setting, and verifies its version before use — no bundled binary. |
| [ADR-016](#adr-016--protocol-compatibility-policy-and-contract-tests) | Protocol compatibility policy and contract tests | Accepted | `SCHEMA_VERSION` bump policy: bump only for a change an existing consumer could misinterpret; additive changes don't bump it. |
| [ADR-017](#adr-017--tracing-for-structured-local-logging-with-centralized-redaction) | `tracing` for structured local logging, with centralized redaction | Accepted | Adopt `tracing` as the structured logging facade; one process-wide subscriber, stderr only, never raw stdout/stderr content. |
| [ADR-018](#adr-018--local-first-privacy-telemetry-opt-in-and-crash-report-consent) | Local-first privacy: telemetry opt-in and crash-report consent | Accepted | GitSail v0.1–v1.0 sends no telemetry and no crash report, period; verified by the absence of any network client dependency. |
| [ADR-019](#adr-019--per-repository-mutation-lock-keyed-by-the-real-git-common-directory) | Per-repository mutation lock, keyed by the real Git common directory | Accepted | `lock_key` resolves the real Git common directory (`git rev-parse --git-common-dir`), correct across linked worktrees. |
| [ADR-020](#adr-020--generic-generation-guarded-cache-performance-budgets-are-measured-never-invented) | Generic generation-guarded cache; performance budgets are measured, never invented | Accepted | `GenerationCache<K, V>` factors out a reusable, bounded, generation-guarded cache pattern; `GraphCache` is its first new consumer. |
| [ADR-021](#adr-021--license-minimum-git-version-rust-msrv-repository-identity-and-pre-10-versioning) | License, minimum Git version, Rust MSRV, repository identity and pre-1.0 versioning | Accepted | Apache License 2.0; minimum Git 2.31; Rust MSRV 1.97.0; repository `rpaggi/gitsail`; Cargo crate versions stay `0.0.0` pre-1.0. |
| [ADR-022](#adr-022--multi-platform-ci-and-architectural-fitness-functions) | Multi-platform CI and architectural fitness functions | Accepted | `.github/workflows/ci.yml`'s five jobs (`rust` matrix, `desktop`, `vscode`, `architecture-fitness`, `dependency-audit`) and what each one checks. |
| [ADR-023](#adr-023--github-releases-only-distribution-no-code-signing-or-marketplaceopen-vsx-publishing-yet) | GitHub Releases-only distribution; no code signing or Marketplace/Open VSX publishing yet | Accepted | `.github/workflows/release.yml` builds CLI/TUI archives, Desktop installers, and a VS Code `.vsix` on every `vX.Y.Z` tag, checksums them, and publishes a GitHub Release — signing/notarization and Marketplace/Open VSX publishing are deliberately deferred, pending external certificate/account resources. |
| [ADR-024](#adr-024--desktop-update-checking-is-check-only-never-an-auto-installer-and-the-running-release-tag-is-embedded-at-build-time) | Desktop update checking is check-only, never an auto-installer; the running release tag is embedded at build time | Accepted | Desktop asks GitHub Releases whether a newer tag exists and shows it with a link and `SHA256SUMS.txt` — it never downloads or installs anything. `build.rs` embeds `RELEASE_TAG` as `GITSAIL_APP_VERSION` so a running build can know its own version despite ADR-021 pinning Cargo/`tauri.conf.json` at `0.0.0`. CLI/TUI/VS Code get no update check in this story. |

**On ever splitting this document:** if the SAD and its ADRs are ever separated into different files (e.g. one file per ADR, or a `docs/architecture/adrs/` directory), the original text of every section and every ADR must be preserved verbatim, and ADR numbers must not be reassigned or reused — see `CONTRIBUTING.md`'s "Updating PRD/SAD/ADRs" section for the exact rule this index exists to keep enforceable. Nothing about that split needs to happen now; this paragraph only documents that the door stays open.

## 1. Purpose

This document defines the initial software architecture for GitSail: an open-source Git client ecosystem composed of a shared Rust core, a keyboard-first TUI, a Tauri/Vue desktop application, and a Visual Studio Code extension focused initially on blame and history.

The architecture is intentionally designed so that user interfaces do not implement Git behavior independently. Git concepts, use cases, errors, and structured contracts live below the presentation layer.

## 2. Architectural goals

1. One Git domain shared by all interfaces.
2. Local-first operation without requiring a GitSail account.
3. Replaceable Git provider.
4. Strong separation between domain, application, infrastructure, and presentation.
5. Safe execution of mutating Git operations.
6. Stable machine-readable protocol for external clients.
7. Good performance on large histories through pagination and lazy loading.
8. Cross-platform support for Windows, Linux, and macOS.
9. Testability without requiring a user's real repositories.
10. Avoid coupling GitSail to GitHub, GitLab, or another forge.

## 3. System context

```text
                         Developer
                            |
          +-----------------+------------------+
          |                 |                  |
     GitSail TUI      GitSail Desktop    VS Code Extension
          |                 |                  |
          +----------- Application API --------+
                            |
                       GitSail Core
                            |
                       Git Port
                            |
                     Git CLI Adapter
                            |
                         git(1)
                            |
                    Local Repository

Optional remote integrations (v1.0+)
GitHub / GitLab / Forgejo
```

## 4. Architectural style

GitSail adopts Ports & Adapters (Hexagonal Architecture).

### Domain
Pure GitSail concepts. No shell execution, UI toolkit, filesystem process management, Tauri, Ratatui, or VS Code dependencies.

### Application
Use cases coordinating domain objects and ports.

### Ports
Interfaces describing capabilities required by the application, such as repository inspection and Git mutation.

### Adapters
Concrete implementations such as `GitCliProvider`.

### Presentation
CLI, TUI, Desktop, and VS Code clients.

## 5. Dependency rule

Dependencies point inward.

```text
Presentation ---> Application ---> Domain
                       |
                     Ports
                       ^
                       |
                 Infrastructure
```

The Domain must not import Infrastructure or Presentation.

## 6. Proposed monorepo

```text
gitsail/
├── crates/
│   ├── gitsail-domain/
│   ├── gitsail-application/
│   ├── gitsail-git/
│   ├── gitsail-protocol/
│   ├── gitsail-cli/
│   └── gitsail-tui/
├── apps/
│   ├── desktop/
│   │   ├── src/
│   │   └── src-tauri/
│   └── vscode/
├── fixtures/
│   ├── repositories/
│   └── scenarios/
├── docs/
│   ├── product/
│   ├── architecture/
│   └── adr/
├── scripts/
├── Cargo.toml
├── README.md
├── CONTRIBUTING.md
└── LICENSE
```

## 7. Crate responsibilities

| Crate | Responsibility |
|---|---|
| `gitsail-domain` | Entities, value objects, domain rules and domain errors |
| `gitsail-application` | Use cases and ports |
| `gitsail-git` | Git CLI adapter and Git-specific infrastructure |
| `gitsail-protocol` | Versioned DTOs used across process boundaries |
| `gitsail-cli` | Human CLI and machine-readable CLI |
| `gitsail-tui` | Ratatui presentation and interaction state |

Desktop Rust commands may depend on application/protocol crates but must not contain duplicated Git parsing logic.

## 8. Domain model

### Repository
Represents a discovered Git repository.

Core fields:
- root path
- worktree path
- bare status
- HEAD state
- current branch
- repository identity

### Commit
- full hash
- short hash
- parents
- author
- committer
- author date
- commit date
- subject
- body
- decorations

### Branch
- name
- local/remote type
- target commit
- upstream
- ahead count
- behind count
- current flag

### RepositoryStatus
Contains working tree and index state.

### FileChange
- path
- previous path when renamed
- change type
- index status
- worktree status

### Diff
Collection of files and hunks.

### DiffHunk
- old/new ranges
- lines
- context

### Blame
Collection of `BlameLine`.

### BlameLine
- final line
- original line
- commit
- author
- timestamp
- content

### Stash
- index
- commit
- message
- date

### Remote
- name
- fetch URL
- push URL

## 9. Application use cases

Initial read use cases:
- OpenRepository
- GetRepositoryStatus
- GetCommitHistory
- GetCommit
- ListBranches
- GetDiff
- GetFileBlame

v0.2/v0.3 mutation use cases:
- StageFiles
- UnstageFiles
- CreateCommit
- CheckoutBranch
- CreateBranch
- DeleteBranch
- FetchRemote
- Pull
- Push

v0.5:
- Merge
- Rebase
- CherryPick
- Revert
- Reset
- Stash
- ResolveConflictWorkflow
- ManageWorktrees

## 10. Git port

Conceptual Rust interface:

```rust
pub trait GitRepositoryPort {
    fn discover(&self, path: &Path) -> Result<Repository, GitSailError>;
    fn status(&self, repo: &Repository) -> Result<RepositoryStatus, GitSailError>;
    fn commits(&self, repo: &Repository, query: CommitQuery)
        -> Result<Page<Commit>, GitSailError>;
    fn branches(&self, repo: &Repository)
        -> Result<Vec<Branch>, GitSailError>;
    fn diff(&self, repo: &Repository, request: DiffRequest)
        -> Result<Diff, GitSailError>;
    fn blame(&self, repo: &Repository, file: &Path)
        -> Result<Blame, GitSailError>;
}
```

Mutation capabilities should be separated from read capabilities where practical so read-only consumers do not automatically gain mutation access.

## 11. Git CLI adapter

`GitCliProvider` is the initial adapter.

Rules:
- Execute `git` directly, not through a shell.
- Pass arguments as process arguments.
- Set repository/worktree explicitly.
- Prefer stable machine-readable output.
- Use explicit separators for fields.
- Avoid parsing human-localized output.
- Capture stdout, stderr, exit code, duration, and cancellation state.
- Never expose raw command output directly as domain objects.
- Redact credentials embedded in remote URLs from logs.

## 12. Process execution boundary

A dedicated process runner isolates OS process behavior.

```text
GitCliProvider
      |
 GitProcessRunner
      |
 std::process / tokio::process
      |
     git
```

The runner owns:
- executable discovery
- environment
- arguments
- timeout
- cancellation
- stdout/stderr collection
- exit status
- diagnostic metadata

## 13. Sync vs async

Domain types remain synchronous and runtime-independent.

Infrastructure may use async process execution. Application APIs may expose async use cases where operations can block, especially fetch/pull/push, large logs, blame, and diff.

The architecture must not force Tokio types into the domain.

## 14. Protocol boundary

External clients such as VS Code consume versioned DTOs from `gitsail-protocol`.

```json
{
  "schemaVersion": 1,
  "requestId": "01H...",
  "data": {
    "repository": "/workspace/project",
    "branch": "main"
  }
}
```

Rules:
- `schemaVersion` is mandatory.
- Domain structs are not serialized directly by default.
- Protocol DTOs map explicitly to/from application/domain models.
- Breaking protocol changes require a new schema version.
- Errors use the same envelope concept.
- The exact increment policy, the per-component supported-version matrix, and the compatibility test fixtures are ADR-016 and `docs/architecture/protocol-compatibility.md`.

## 15. Initial CLI protocol

Human mode:

```bash
gitsail log
gitsail status
```

Machine mode:

```bash
gitsail log --json
gitsail blame src/main.rs --json
```

The CLI is both a user interface and the first cross-process integration mechanism.

Longer term, a local daemon/IPC transport may be introduced without changing the application contracts.

## 16. VS Code architecture

```text
VS Code Extension (TypeScript)
          |
   GitSail Client
          |
 CLI JSON initially
          |
 gitsail-protocol
          |
 Application/Core
```

The extension owns:
- VS Code decorations
- hover UI
- commands
- configuration
- editor lifecycle

It does not own:
- Git log parsing
- blame parsing
- repository discovery rules
- Git mutation semantics

## 17. Desktop architecture

```text
Vue 3 UI
   |
Typed frontend service
   |
Tauri Commands
   |
gitsail-application
   |
Git Port
```

Tauri commands should be thin adapters. Business logic belongs in application/domain crates.

Desktop state is divided into:
- view state
- repository session state
- application data
- persisted preferences

## 18. TUI architecture

Use a unidirectional event/update/render model.

```text
Terminal Events
      |
    Action
      |
    Update
      |
 App State
      |
    Render
```

Long-running Git operations run outside the render loop and return typed messages/events.

## 19. Error model

Top-level categories:
- RepositoryNotFound
- GitNotInstalled
- UnsupportedGitVersion
- InvalidRepositoryState
- OperationConflict
- AuthenticationRequired
- PermissionDenied
- NetworkFailure
- ProcessFailure
- ParseFailure
- Timeout
- Cancelled
- ProtocolMismatch
- Internal

Errors contain:
- stable error code
- user-safe message
- optional remediation
- diagnostic cause
- operation ID

Raw stderr may be retained for diagnostics but must not automatically be shown as the primary UX message.

## 20. Safety model for mutations

Mutating operations include metadata describing risk.

```text
Safe:
fetch, stage, unstage

Moderate:
commit, checkout, merge

Destructive:
reset --hard, discard changes, force push, stash drop
```

Presentation layers decide how confirmation is rendered, but the application layer supplies operation intent and risk metadata.

Destructive operations must not be silently downgraded into generic actions.

## 21. Repository session

A repository session represents active application context.

Responsibilities:
- canonical repository path
- HEAD snapshot
- working tree status snapshot
- selected branch/commit
- refresh generation
- active operations

Sessions must tolerate external Git changes made by another terminal/editor.

## 22. Refresh strategy

GitSail assumes repositories can change outside the application.

Initial strategy:
- refresh after GitSail mutation
- refresh on focus
- manual refresh
- optional filesystem watching

Future optimization may use debouncing and selective invalidation.

## 23. Caching

Cache is an optimization, never the source of truth.

Candidates:
- commit pages
- graph layout
- avatar metadata
- expensive blame results

Do not cache:
- destructive operation eligibility without revalidation
- authentication state indefinitely
- working tree state without invalidation

## 24. Commit graph architecture

Graph rendering is separate from commit retrieval.

```text
Commit DAG
   |
Graph Layout Engine
   |
Graph Rows/Lanes
   |
TUI Renderer / Desktop Renderer
```

This allows TUI and Desktop to share graph semantics while rendering differently.

Graph layout inputs:
- commit hash
- parent hashes
- decorations
- pagination boundary metadata

## 25. Pagination

History must never assume the entire repository history fits in memory.

`CommitQuery` supports:
- limit
- cursor
- revision range
- branch/ref
- author
- text search where supported

The application returns a page with continuation metadata.

## 26. Concurrency

Rules:
- UI thread/render loop never waits on Git process execution.
- Mutations against the same repository are serialized unless proven safe.
- Read operations may run concurrently.
- A mutation triggers invalidation of relevant read caches.
- Cancellation tokens should be supported for expensive reads.

## 27. Authentication

GitSail initially delegates Git credentials to existing Git mechanisms:
- Git Credential Manager
- SSH agent
- OS keychain integrations used by Git
- configured credential helpers

GitSail must not introduce its own plaintext credential store.

Forge API integrations later use OS-secure credential storage or provider-supported auth flows.

## 28. Logging and diagnostics

Structured local logs:
- timestamp
- level
- component
- operation ID
- duration
- safe command metadata
- error code

Never log:
- access tokens
- passwords
- credential-bearing remote URLs
- complete file contents by default

## 29. Configuration

Configuration scopes:
1. Built-in defaults.
2. User preferences.
3. Repository-specific GitSail preferences where justified.

Do not modify `.git/config` for UI-only preferences.

Potential shared preferences:
- date format
- graph behavior
- default diff mode
- confirmation policy within safe bounds

## 30. Cross-platform rules

- Use Rust `Path`/`PathBuf`; never assume `/`.
- Test Windows drive paths and UNC paths.
- Preserve non-UTF-8 path limitations explicitly where relevant.
- Avoid shell-specific syntax.
- Do not assume case-sensitive filesystem.
- Support standard Git executable discovery plus explicit override.

## 31. Testing architecture

### Unit
Domain and pure graph/layout logic.

### Contract
Every Git provider must satisfy common behavioral tests.

### Integration
Temporary real Git repositories created during tests.

### Fixtures
Scenarios:
- empty repository
- one commit
- multiple branches
- merge commit
- detached HEAD
- dirty tree
- rename
- binary file
- merge conflict
- rebase in progress
- shallow clone
- bare repository

### UI
- TUI state/update tests
- Desktop critical E2E
- VS Code extension tests

## 32. Performance budgets

Initial engineering targets, subject to benchmark refinement:
- CLI startup should feel effectively immediate on common repositories.
- First history page should avoid scanning full history.
- UI must remain responsive during Git operations.
- Graph rendering must be incremental.
- Large diffs and blame results should support cancellation.

No hard millisecond SLA is frozen before representative benchmarks exist.

## 33. Security considerations

Primary threat areas:
- command injection
- malicious repository paths
- malicious commit messages/file names
- credential leakage
- unsafe remote URLs
- destructive Git actions
- untrusted forge API content

All repository text displayed in terminal or GUI is treated as untrusted input. Terminal escape/control sequences must be sanitized before rendering.

## 34. Extension points

Post-v1 candidates:
- alternate Git provider
- forge providers
- plugin API
- local daemon
- JetBrains client
- Neovim client

Extension points should be introduced only after a real second implementation exists or a clear boundary is already required.

## 35. Build and CI

CI matrix:
- Linux
- Windows
- macOS

Pipeline stages:
- format
- lint
- unit tests
- integration tests
- protocol compatibility tests
- build CLI/TUI
- desktop build smoke test
- VS Code extension test/build
- security/dependency audit where practical

## 36. Release architecture

Artifacts may include:
- `gitsail` CLI/TUI binary
- GitSail Desktop installers/packages
- GitSail VS Code extension

Versioning should keep protocol compatibility explicit even if UI applications have different release cadence later.

## 37. Architecture fitness rules

Automated or review-enforced constraints:
- domain cannot depend on Tauri/Ratatui/Node.
- presentation cannot execute raw Git commands.
- Git parsing stays in infrastructure.
- external protocol does not expose internal structs accidentally.
- destructive actions have typed intent.
- new providers pass provider contract tests.

## 38. Initial implementation sequence

```text
1. Workspace + gitsail-domain
2. Error model + repository entities
3. gitsail-application ports/use cases
4. GitProcessRunner
5. GitCliProvider repository discovery
6. status
7. log + pagination
8. branches
9. diff
10. blame
11. gitsail-protocol
12. gitsail-cli --json
13. integration fixtures
14. architecture review
15. v0.1 release
```

No TUI implementation is required to prove the v0.1 architecture.

# Architecture Decision Records

## ADR-001 — Rust for the shared core

**Status:** Accepted

### Context
GitSail requires a cross-platform, performant core usable by CLI, TUI and Desktop, with strong modeling and process control.

### Decision
Use Rust for domain, application, Git infrastructure, protocol support, CLI and TUI.

### Consequences
Positive:
- strong type system
- native binaries
- good process/filesystem APIs
- Ratatui ecosystem
- direct Tauri integration
- predictable resource usage

Trade-offs:
- higher learning curve
- compile times
- VS Code still requires a TypeScript boundary

---

## ADR-002 — Ports & Adapters architecture

**Status:** Accepted

### Context
GitSail must support multiple interfaces without duplicating Git logic and may replace its Git implementation later.

### Decision
Adopt Ports & Adapters with Domain, Application, Ports, Infrastructure Adapters and Presentation.

### Consequences
The initial codebase has more boundaries than a small CLI, but enables independent interfaces, test doubles and future providers.

---

## ADR-003 — Git CLI as initial Git provider

**Status:** Accepted

### Context
Implementing Git internals or adopting a native Git library immediately would increase scope before product behavior is validated.

### Decision
Use the installed `git` executable through a dedicated adapter.

### Consequences
Positive:
- mature Git behavior
- compatibility with user credentials/configuration
- faster v0.1

Trade-offs:
- process overhead
- careful parsing required
- Git must be installed
- behavior may vary across Git versions

---

## ADR-004 — Monorepo

**Status:** Accepted

### Context
Core, TUI, Desktop and VS Code share contracts and evolve together.

### Decision
Keep GitSail components in one repository initially.

### Consequences
Atomic changes across protocol and clients are easier. CI is more complex but centralized.

---

## ADR-005 — Ratatui for TUI

**Status:** Accepted

### Context
The TUI is a first-class GitSail interface and must integrate naturally with the Rust core.

### Decision
Use Ratatui for terminal rendering and interaction.

### Consequences
The TUI remains native Rust and can reuse domain/application types without IPC.

---

## ADR-006 — Tauri + Vue 3 for Desktop

**Status:** Accepted

### Context
GitSail needs a rich desktop UI without adopting a full Electron runtime.

### Decision
Use Tauri as desktop shell/backend integration and Vue 3 for the frontend.

### Consequences
Positive:
- Rust integration
- web UI productivity
- relatively lightweight distribution

Trade-offs:
- WebView differences across platforms
- frontend/backend bridge must remain thin

---

## ADR-007 — TypeScript for VS Code extension

**Status:** Accepted

### Context
VS Code extensions use the Node/TypeScript ecosystem and editor APIs.

### Decision
Implement the extension in TypeScript and consume GitSail through a structured external boundary.

### Consequences
A process/protocol boundary is required, but editor integration remains idiomatic.

---

## ADR-008 — Versioned structured protocol

**Status:** Accepted

### Context
VS Code and potentially other clients cannot directly share Rust memory/types.

### Decision
Define explicit versioned DTOs in `gitsail-protocol`; use JSON as the initial serialization format.

### Consequences
Adds mapping code but prevents accidental coupling and permits future IPC transport changes.

---

## ADR-009 — Separate read and mutation capabilities

**Status:** Accepted

### Context
Some clients primarily inspect repositories while mutations carry greater risk.

### Decision
Separate read-oriented and mutation-oriented ports/use cases where practical.

### Consequences
Capabilities become clearer, testing is easier, and clients can depend on narrower APIs.

---

## ADR-010 — GitSail does not own credentials

**Status:** Accepted

### Context
Git authentication is security-sensitive and mature credential mechanisms already exist.

### Decision
Delegate Git transport credentials to Git/SSH/OS credential helpers. Future forge tokens use secure OS storage.

### Consequences
GitSail avoids a sensitive credential subsystem but must provide useful diagnostics when external credential flows fail.

---

## ADR-011 — Graph layout separated from rendering

**Status:** Accepted

### Context
TUI and Desktop both need a commit graph but render with different technologies.

### Decision
Implement graph semantics/layout independently of UI rendering.

### Consequences
Graph behavior can be shared and tested while each interface retains visual freedom.

---

## ADR-012 — CLI JSON before daemon/IPC

**Status:** Accepted

### Context
VS Code needs access to the Rust core, but building a daemon before validating the contract adds complexity.

### Decision
Use CLI JSON as the first cross-process transport. Revisit local daemon/IPC when latency or lifecycle requirements justify it.

### Consequences
Simple distribution and debugging initially, at the cost of process startup overhead.

## ADR-013 — clap for CLI argument parsing

**Status:** Accepted

### Context
EPIC-08 needed a parser for the CLI's global flags, six subcommands and their per-command options/help text (US-036). Hand-rolling one would duplicate a well-solved problem; `gitsail-application`/`gitsail-domain` deliberately stay dependency-free, but the CLI crate is a presentation-layer binary with no such constraint.

### Decision
Use `clap` (derive API) for `gitsail-cli`. Usage errors (missing/unknown arguments, bad subcommands) exit with clap's own code `2` and message, kept distinct from the domain-error exit codes in `exit_code.rs`.

### Consequences
Consistent, discoverable `--help` text (including examples) essentially for free; adds one dependency tree to the CLI binary only, never to `gitsail-domain`/`gitsail-application`/`gitsail-git`.

## ADR-014 — Protocol envelope, correlation and cursor shape

**Status:** Accepted

### Context
ADR-008 established that a versioned envelope exists; it did not fix the exact JSON shape, and SAD §39 left "exact protocol envelope and cursor representation" open pending a concrete consumer (US-035, US-037).

### Decision
`gitsail-protocol::Envelope<T>` is tagged by a `status` field (`"ok"` or `"error"`), always carries `schemaVersion` (currently `1`) and a `requestId` (an opaque, process-generated correlation string), and holds either `data: T` or `error: ErrorPayload` — never both. Pagination reuses the same `nextCursor`/`hasMore` shape as `gitsail-application::Page`, with the cursor kept as an opaque string (the Git CLI adapter currently encodes it as a decimal offset, but consumers must not parse it).

### Consequences
Consumers (the CLI's own `--json` mode today, VS Code/Desktop later) branch on `status` alone and never need to guess the wire shape; a future breaking change increments `schemaVersion` (US-039) rather than being inferred from field presence.

## ADR-015 — VS Code binary distribution for v0.4

**Status:** Accepted

### Context
EPIC-14/US-070 requires a registered decision on how the VS Code extension obtains the `gitsail-cli` binary it depends on (ADR-012), before the extension is packaged for v0.4. Two options exist: bundle a per-platform `gitsail-cli` build inside the `.vsix`, or require the user to install `gitsail` separately (PATH or an explicit configured path). EPIC-25 (Distribution & Updates) — the epic that would define a signed, per-OS/arch release pipeline — has not shipped at this point in the roadmap (v0.4), so there is no artifact this extension could bundle with verifiable origin.

### Decision
For v0.4, the VS Code extension does not bundle a `gitsail-cli` binary. It discovers `gitsail`/`gitsail.exe` on `PATH` by default, or uses an explicit `gitsail.binaryPath` setting (respected only in a trusted workspace — SAD §33). Before use, the extension spawns `--version` and compares the result against a minimum supported version, refusing to proceed silently on a missing or unrecognized binary rather than falling back to any alternate parsing. This mirrors ADR-010's stance ("GitSail avoids owning a sensitive subsystem it cannot yet do safely, but gives useful diagnostics") applied to binary provenance instead of credentials.

### Consequences
Users must install `gitsail` themselves before the extension is useful — a real onboarding cost, documented in `apps/vscode/README.md`. In exchange, the extension never silently trusts or executes a binary of unknown origin. This decision is revisited once EPIC-25 defines a signed-artifact pipeline the extension can safely embed per platform.

## ADR-016 — Protocol compatibility policy and contract tests

**Status:** Accepted

### Context
ADR-014 fixed the envelope's JSON shape and named `schemaVersion` as the mechanism for a future breaking change, but left "when exactly must `SCHEMA_VERSION` increment" and "how is that enforced" as prose rather than a checked policy (US-039). The VS Code extension (US-069/ADR-015-era work) already implemented a strict consumer-side guard — `apps/vscode/src/protocol.ts`'s `parseEnvelope` rejects an unrecognized `schemaVersion` before ever reading `data`/`error` — but nothing on the Rust producer side had an equivalent, testable guard, and no document recorded which component supports which `schemaVersion` for a given release.

### Decision
1. `gitsail_protocol::SCHEMA_VERSION`'s doc comment (`crates/gitsail-protocol/src/envelope.rs`) states the increment policy explicitly: bump it for any change an unmodified existing consumer could misinterpret (field/variant rename or removal, type change, re-tagging, or — because these DTO enums use no `#[serde(other)]` fallback — adding a new variant to an already-shipped enum); do not bump it for a genuinely additive change (new optional field, new DTO type) existing consumers already tolerate by ignoring unknown JSON keys.
2. `crates/gitsail-protocol/tests/dto_wire_shape.rs` pins the exact serialized shape of one representative DTO per `#[serde(...)]` pattern in use, so a silent wire-shape change (e.g. dropping a `rename_all`, changing a `tag`) fails a test at the point of change rather than shipping unnoticed — forcing whoever touches a DTO to consciously apply policy 1.
3. `gitsail_protocol::compat` (`parse_envelope`, `SUPPORTED_SCHEMA_VERSIONS`, `EnvelopeDecodeError`) gives Rust the same two-phase, reject-before-reading-`data` guard `protocol.ts` already has, for any future Rust consumer that crosses an independently-versioned process boundary (a daemon/IPC transport per ADR-012's "open" item). Today's in-process Rust consumers (TUI, Desktop) do not call it: neither crosses such a boundary — see `docs/architecture/protocol-compatibility.md`'s evaluation.
4. `docs/architecture/protocol-compatibility.md` records, per release, which `schemaVersion`(s) each component (`gitsail-cli`, VS Code, TUI, Desktop) supports, and points at the fixtures in `docs/architecture/fixtures/protocol-compatibility/` shared by the Rust (`crates/gitsail-protocol/tests/compatibility.rs`, `crates/gitsail-cli/tests/protocol_compatibility.rs`) and TypeScript (`apps/vscode/test/protocolCompatibility.test.ts`) compatibility suites, including one real-producer/real-consumer end-to-end test and one hypothetical-incompatible-version rejection test.

### Consequences
Changing a DTO's wire shape now fails a fast, local test instead of only surfacing when a mismatched client/CLI pair meets in the field. The matrix and fixtures are release artifacts a maintainer updates deliberately (documented in the matrix's own "when this table must change" section) rather than being inferred after the fact. No actual schema change ships with this decision — `SCHEMA_VERSION` stays `1`; the hypothetical incompatible fixture (`unsupported-v2.json`) is explicitly synthetic, invented only to exercise the rejection path.

## ADR-017 — `tracing` for structured local logging, with centralized redaction

**Status:** Accepted

### Context
SAD §28 already specified the shape of GitSail's local logs (level, component, operation ID, duration, safe command metadata, error code) and what must never appear in them (tokens, passwords, credential-bearing URLs, full file contents). No logging infrastructure existed yet (EPIC-22/T-224/US-113): `GitCliProvider`/`GitProcessRunner` only ever raised `GitSailError`s, and `gitsail-cli`'s `--debug` flag printed ad hoc `eprintln!` lines with no level/component/duration structure and no reusable redaction beyond a single URL-shaped-argument helper.

### Decision
Adopt `tracing` (not `log`) as the workspace's structured logging facade, since its field-based events (`tracing::debug!(component = ..., operation = ..., duration_ms = ..., error_code = ..., "...")`) map directly onto SAD §28's required log shape without string formatting, and its ecosystem (`tracing-subscriber`) is the idiomatic choice for a Rust binary that wants an install-once global subscriber. `gitsail-git`'s `GitProcessRunner::run_process` emits one event per invocation with exactly that field set — component, operation (the `git` subcommand only, already redacted), duration, exit/error code — and never raw stdout/stderr. `gitsail-cli` installs the one process-wide subscriber (`WARN` by default, `DEBUG` with `--debug`), writing to stderr only, alongside the existing `--debug` diagnostic output.

Redaction itself is centralized in `gitsail_domain::redact` (`redact_credential_url` for a single URL-shaped argument, `redact_secrets` for free-form text such as `git`'s own stderr, covering credential-bearing URLs, `token=`/`password=`/`secret=`-shaped fragments, and `Authorization: <scheme> <token>` headers) rather than reimplemented per call site. `gitsail-git`'s `ProcessDiagnostic` now redacts `stderr` the same way it already redacted `args`, closing a gap where a credential embedded in Git's own error text (not just in the argument list) could have reached a diagnostic.

### Consequences
Positive: one dependency-light facade shared by every crate that needs to log (currently `gitsail-git`, `gitsail-cli`); verbosity (`--debug`) can never bypass redaction, because every event's fields are already redacted before being emitted, not filtered afterward. `tracing`/`tracing-subscriber` are new dependencies, added to `gitsail-git` and `gitsail-cli` only (never `gitsail-domain`/`gitsail-application`, preserving SAD §4's dependency-free-core rule). `gitsail-tui`/`apps/desktop`/`apps/vscode` do not install a subscriber as part of this decision — a future story wiring TUI/Desktop-side logging can reuse the same `gitsail_domain::redact` module and, if useful, install their own `tracing` subscriber without any change to how events are emitted deeper in the stack.

## ADR-018 — Local-first privacy: telemetry opt-in and crash-report consent

**Status:** Accepted

### Context
SAD §28/§33 and ADR-010 already established that GitSail is local-first and does not own a credential store; EPIC-22/T-225/US-114 asked for the same explicitness about *usage telemetry* and *crash reporting*, neither of which is implemented yet. Without a recorded decision, a future contributor could add either with an accidental default-on behavior, or with no consent gate at all.

### Decision
1. GitSail v0.1–v1.0 sends no telemetry and no crash report, period — confirmed by the absence of any HTTP/network client dependency in any crate's `Cargo.toml` in this workspace, and exercised by a factory-default-configuration test (`gitsail_application::privacy`).
2. `gitsail_application::privacy::TelemetryPreference` and `CrashReportConsent` are reserved, explicit configuration types whose `Default` resolves to `Disabled`/`NotGranted`. If telemetry or crash-report upload is ever implemented, it must read one of these preferences and must not transmit anything unless the person explicitly opted in; opting in must be a documented, discoverable action, never a pre-ticked box or an inferred consent from continued use.
3. A local crash (a Rust panic) is always logged locally — `gitsail-cli` installs a panic hook that logs through the same redacted, structured `tracing` path ADR-017 describes, then still runs Rust's default panic hook (preserving the usual stderr message/backtrace) — but this is strictly local file/stderr output. Nothing about local panic logging transmits a report anywhere; that would require the explicit consent policy 2 already imposes.
4. This ADR does not itself add a telemetry or crash-reporting *implementation* — it fixes the policy such an implementation must follow, ahead of it existing, so the default is never accidentally "on."

### Consequences
Positive: a future telemetry/crash-reporting story has an unambiguous bar to clear (explicit opt-in, documented, revocable) rather than needing to invent the policy under implementation pressure; local diagnosis (via `--debug` and local logs) never requires any upload, matching ADR-010's "useful diagnostics without owning a sensitive subsystem" stance applied to telemetry instead of credentials. Trade-off: until a real telemetry story exists, `TelemetryPreference`/`CrashReportConsent` have no reader besides their own tests — they are a deliberately inert placeholder, not a feature.

## ADR-019 — Per-repository mutation lock, keyed by the real Git common directory

**Status:** Accepted

### Context
SAD §26 states the policy in prose ("mutations against the same repository are serialized unless proven safe") without naming a mechanism. EPIC-23/T-227/US-116 asked for a concrete one in Core, so `gitsail-tui` and `apps/desktop` do not each hand-roll their own — `apps/desktop/src-tauri/src/state.rs` already has a single-instance `epoch`/`Mutex<SessionSlot>` pattern for exactly this kind of coordination, but it is scoped to one process's one open repository, not generalized, and not shared with the CLI or TUI.

The open question this ADR resolves: **what identifies "the same repository" for locking purposes?** `Repository::root_path` (the worktree toplevel) is the obvious candidate, but `git worktree` lets several worktrees share one object database, ref namespace, and (depending on Git version and exact operation) index/lock namespace while each reports a *different* `root_path`. Verified directly against Git 2.43 behavior: `git rev-parse --git-common-dir` resolves every linked worktree of a repository to the same absolute path, while `git rev-parse --show-toplevel` (which `Repository::root_path` is built from) differs per worktree, confirming the two worktrees really do share one underlying repository identity distinct from either one's own working directory.

### Decision
1. `RepositoryReadPort` (`crates/gitsail-application/src/ports.rs`) gains a `lock_key(&self, repo: &Repository) -> Result<PathBuf, GitSailError>` method with a default implementation returning `repo.root_path.clone()` — correct for the common case (a repository with no linked worktrees) and requiring no change to any existing test double. `GitCliProvider` (`crates/gitsail-git/src/provider.rs`) overrides it to resolve `git rev-parse --git-common-dir` (falling back to `--absolute-git-dir` for a pre-2.5 Git, which lacks `--git-common-dir` but is still correct for the no-linked-worktrees case).
2. `gitsail-application::concurrency` introduces `RepositoryLockRegistry`: a process-wide map from a resolved lock key to a shared `Arc<Mutex<()>>`, exposed through one static (`global_lock_registry()`). `RepositorySession::run_mutation` resolves (and caches) its session's lock lazily on first use, holds it only for the duration of the caller-supplied mutation closure, then — on success only — runs an `AfterMutation` refresh and invalidates every cache registered via `RepositorySession::register_cache`.
3. This lock is deliberately coarser than Git's own actual locking granularity: since Git 2.5, each linked worktree has its *own* index file (so two worktrees can, in principle, stage/commit concurrently without touching the same `index.lock`), while ref/config mutations do go through the shared common directory. Rather than special-case "this mutation only touches this worktree's own index" vs. "this mutation touches shared refs," every mutation type is serialized at the coarser, always-safe common-directory granularity. This trades a small amount of theoretically-safe cross-worktree parallelism (e.g. two worktrees staging files at the same instant) for a single, simple, always-correct rule with one lock key resolution path.
4. This registry only ever coordinates **within one process**. It is not a substitute for Git's own `.git/index.lock`, which is the real cross-process guard (another `git` invocation, from any tool, on any worktree of the repository). When GitSail's own mutation loses that race anyway, `gitsail-git::provider::classify_index_lock_conflict` reclassifies Git's refusal as `ErrorCode::RepositoryLocked` — a clear, actionable error — rather than GitSail ever waiting for or removing another process's lock file itself (doing so could corrupt state a still-live process is writing).

### Consequences
Positive: `gitsail-tui` and `apps/desktop` gain correct multi-worktree-aware mutation serialization without either implementing it themselves — `apps/desktop/src-tauri/src/state.rs`'s existing single-instance `epoch` mechanism is a reasonable candidate to eventually delegate to this Core primitive instead of keeping its own parallel implementation, though this ADR does not perform that migration (out of scope for EPIC-23; a future task can cite this ADR). A repository with no linked worktrees pays no extra cost (the default `lock_key` needs no adapter round-trip). Trade-off: resolving `git rev-parse --git-common-dir` is one extra `git` process invocation the first time a given session mutates (amortized after that, since the result is cached for the session's lifetime); and the "coarser than strictly necessary" locking (decision 3) means two worktrees can never mutate the same physical repository concurrently through GitSail even in the cases upstream Git itself would tolerate — an explicit, documented trade favoring simplicity and safety over maximum parallelism.

## ADR-020 — Generic generation-guarded cache; performance budgets are measured, never invented

**Status:** Accepted

### Context
SAD §23 states caching policy in prose ("cache is an optimization, never the source of truth"; candidates include "commit pages" and "graph layout") without a shared mechanism. `gitsail-application::blame_cache::BlameCache` (EPIC-07/US-034) already implements exactly the right shape for one cache — key by everything that could make two queries differ, guard late results with a generation ticket, invalidate wholesale on a relevant change — but as one hand-written, blame-specific module. EPIC-23/T-228/US-117 asked for the commit graph (US-065, `gitsail_domain::graph::CommitGraph`) to get the same treatment, and separately, SAD §32 forbids freezing a numeric performance SLA before real measurements exist (EPIC-23/T-226/T-229 asked for exactly those measurements).

### Decision
1. `gitsail-application::cache::GenerationCache<K, V>` factors `BlameCache`'s pattern into a reusable, bounded, generation-ticket-guarded map. `BlameCache` itself is left as its own already-correct, already-tested implementation rather than migrated onto `GenerationCache` — a mechanical rewrite of a working module for no behavioral gain, which US-117's own wording explicitly allows skipping when disproportionate. `gitsail-application::graph_cache::GraphCache` is the new consumer built directly on `GenerationCache`, keyed by `GraphPageKey { repository, filter_signature, cursor }` (repository/filter/page, per US-117 criterion 1) and bounded by `DEFAULT_GRAPH_CACHE_CAPACITY` so it never grows without bound.
2. Both `BlameCache` and `GraphCache` implement `gitsail_application::concurrency::Invalidatable`, so `RepositorySession::run_mutation` can drive every registered cache's invalidation through one uniform call after a successful mutation (T-227/US-116 criterion 3), instead of each mutation use case needing to know the full list of caches that might now be stale.
3. Cache is never consulted for a destructive/risky decision: `RepositoryWritePort::amend_commit`'s `expected_head` revalidation already checks a fresh `resolve_revision` call against the real repository, never a cached value — this ADR records that as the explicit, binding principle for any future cache-adjacent mutation, not merely an incidental property of `amend_commit`'s current implementation.
4. `docs/architecture/performance-baseline.md` (T-226/T-229) records this epic's actual measurements (small/medium/large synthetic fixtures) and derives regression budgets *from those numbers*, not ahead of them — consistent with SAD §32's explicit "no hard millisecond SLA is frozen before representative benchmarks exist." That document, not this ADR, is the place those budgets live and get updated as new measurements arrive.

### Consequences
Positive: a future cache (e.g. for diff results, another SAD §23 candidate) has a ready-made, tested primitive instead of a third hand-rolled copy of the same ticket/eviction logic; `RepositorySession` gained one small, generic seam (`register_cache`) rather than needing to know about `BlameCache` and `GraphCache` as concrete types. Trade-off: `GenerationCache`'s eviction is a simple FIFO bound, not a recency-aware LRU — acceptable for the DoD this epic actually requires (bounded growth, proven by a test) but a candidate for refinement if a real access pattern later shows FIFO evicting entries that are still frequently reused.

## ADR-021 — License, minimum Git version, Rust MSRV, repository identity and pre-1.0 versioning

**Status:** Accepted

### Context
§22/§39 of this baseline (and the backlog's §8 "Registro de decisões abertas e limites") listed five decisions as open before any public release: MIT vs Apache-2.0, minimum supported Git version, Rust MSRV, the repository's name/organization, and the pre-1.0 versioning policy (EPIC-26/T-263/US-130). No LICENSE file existed; no crate declared `rust-version`; no document stated which Git version the CLI adapter actually requires; the repository already has a real `origin` remote; and every workspace crate has stayed at Cargo version `0.0.0` since ADR-013 (confirmed still true in `docs/architecture/protocol-compatibility.md`).

### Decision
1. **License: Apache License 2.0.** `LICENSE` (repository root) now carries the full Apache-2.0 text. Chosen over MIT for its explicit patent grant (§3) and patent-litigation retaliation clause (§3, last sentence) — relevant for a developer-tooling project where contributors and downstream integrators (e.g. a future VS Code marketplace listing, ADR-015) benefit from an explicit patent license, not just a copyright one. This is a preference between two permissive licenses with no known project-specific blocker to either; Apache-2.0 requiring a `NOTICE`-file mechanism and stating modified-file markers explicitly (§4(b)) is treated as a minor, acceptable maintenance cost, not a rejection reason.
2. **Minimum supported Git version: 2.31.** Verified empirically by grepping every literal Git argument `gitsail-git` already passes (`crates/gitsail-git/src/provider.rs`), not assumed: `rev-parse --path-format=absolute` (`discover`, `git_common_dir`, `absolute_git_dir`) is the strictest requirement found — that flag was added in Git 2.31 (2021). `--end-of-options` (`resolve_revision`, `fast_forward_merge`, log/rev-parse revision boundaries) needs only Git 2.24, and the explicit two-part `--force-with-lease=<branch>:<expected>` form (`force_push_with_lease`) needs only Git 1.8.5 — both already satisfied by the 2.31 floor. The Git installed in this development environment (`git --version` → 2.43.0) exceeds the floor, so nothing here required lowering behavior to match a decision; the decision only records, for the first time, the actual constraint the existing code already imposes.
3. **Rust MSRV: 1.97.0.** `rust-version = "1.97.0"` is now set in `[workspace.package]` (root `Cargo.toml`) and inherited (`rust-version.workspace = true`) by every workspace member (`gitsail-domain`, `gitsail-application`, `gitsail-git`, `gitsail-protocol`, `gitsail-cli`, `gitsail-tui`, `apps/desktop/src-tauri`); `cargo build --workspace` passes with this pin in place. This is a **floor-equals-current** pin, not a compatibility-tested lower bound: `cargo --version`/`rustc --version` both report `1.97.0` in the only environment this workspace has ever been built in, and no older toolchain has been tried against it. It must not be lowered without actually building the workspace on that older toolchain first — this ADR deliberately avoids inventing a smaller number on the assumption that "it probably still compiles."
4. **Repository name/organization: `rpaggi/gitsail` (GitHub), unchanged for now.** `git remote -v` already resolves `origin` to `git@github.com:rpaggi/gitsail.git`; this ADR records that as the current, de facto repository identity rather than inventing a different name or a not-yet-created organization. Moving to a dedicated GitHub organization is left as a future, explicit decision to make only when the project is ready for broader/shared maintainership — not a prerequisite for v0.1.
5. **Pre-1.0 versioning policy: Cargo crate versions stay at `0.0.0`.** Every workspace crate (`gitsail-domain`, `gitsail-application`, `gitsail-git`, `gitsail-protocol`, `gitsail-cli`, `gitsail-tui`, `gitsail-desktop`) keeps `version = "0.0.0"` through every pre-1.0 milestone (v0.1 … v0.5). Milestone identifiers (`v0.1`, `v0.2`, …, `v1.0`) are tracked as git tags/release notes, distinct from Cargo's own semver, exactly as ADR-016's protocol-compatibility matrix already tracks `schemaVersion`/CLI-binary version independently of any crate's Cargo version. The first real Cargo semantic version (`0.1.0` or `1.0.0`) is an explicit decision to make at the first actual public release (US-130 DoD: "revisados antes de qualquer distribuição"), not before — this ADR does not pre-assign that number.

### Consequences
Positive: a contributor or downstream packager now has one authoritative place (`LICENSE`, this ADR, and the updated backlog decision table — docs/product/GitSail_Product_Backlog_v1.0.md §8) instead of an open question repeated across PRD §22/SAD §39/backlog §8. Verified-floor MSRV and the Git-version floor are both derived from evidence already in this repository (installed toolchain, and `provider.rs`'s own arguments) rather than picked arbitrarily. Trade-off: the MSRV floor is only as trustworthy as "the one toolchain this has ever been built with" — it is expected to be revisited (likely lowered, after real testing) once CI exists and can matrix-test older Rust releases; this ADR does not claim that testing has happened. Likewise, trademark/brand availability for the name "GitSail" is explicitly **not** evaluated or asserted here — see `docs/product/brand-identity.md` (US-129) — choosing Apache-2.0 says nothing about that separate, unresolved question.

## ADR-022 — Multi-platform CI and architectural fitness functions

**Status:** Accepted

### Context
No CI existed in this repository before T-254/US-121 (`.github/workflows/` did not exist): every check — formatting, lint, tests, build, and any architectural rule such as "domain must not depend on infrastructure" or "only `gitsail-git` shells out to `git`" — was enforced only by convention and manual review. Section 40's "architecture definition of done for v0.1" already listed "Windows/Linux/macOS CI passes" as a completion bar for v0.1, unmet until now. `apps/desktop/src-tauri` and `gitsail-tui` are already regular members of the root workspace `Cargo.toml`, and `apps/vscode` already exists, so a CI baseline scoped to "just CLI + Core" would have ignored components already present in the tree.

### Decision
1. `.github/workflows/ci.yml` adds five jobs: `rust` (matrix `ubuntu-latest`/`windows-latest`/`macos-latest`, running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `cargo build --workspace` — this covers the whole Rust workspace, not just CLI/Core, since TUI and Desktop's Rust side are already workspace members), `desktop` and `vscode` (Node/TS builds and tests, `ubuntu-latest` only — documented as a deliberate cost trade-off, not an oversight, in `docs/architecture/ci-policy.md`), `architecture-fitness` (see below), and `dependency-audit` (`cargo audit`/`npm audit`, informative only for this first version).
2. `scripts/ci/check-architecture.sh` implements two approximate, grep/text-based architectural fitness functions: `gitsail-domain`'s `Cargo.toml` must not list any other GitSail workspace crate as a dependency (domain isolation, ADR-002), and no `*.rs` file outside `crates/gitsail-git/**` may construct a subprocess literally named `"git"` (restricted parsing), with documented, reviewed exceptions for test-only fixture code. The protocol/`SCHEMA_VERSION` check US-121 also asks for is not reimplemented — T-172/ADR-016's existing compatibility tests already run inside `cargo test --workspace`.
3. `docs/architecture/ci-policy.md` is the single source of truth for which checks are required for merge vs. informative, and records this pipeline's explicit assumptions (GitHub-hosted runners ship `git`; Tauri v2 needs Linux-only system packages) and its sandbox-verification limits (every command the workflow runs was run directly on Linux before this file existed; the workflow's own YAML syntax was checked with `actionlint` and `python3 -c "import yaml"`, but no live GitHub Actions run — on any OS — was possible from this environment).

### Consequences
Positive: v0.1's "Windows/Linux/macOS CI passes" completion bar (§40) is now met; two real, previously-undetected architectural rules get automated enforcement instead of relying on review; a first-ever dependency audit exists and is visible in every run's logs even before it blocks anything. Trade-off: the fitness functions are intentionally shallow (text/grep-based, with an explicit allowlist for one file a plain grep cannot parse `#[cfg(test)]` boundaries in) rather than a true architectural-conformance tool — acceptable for a first version per US-121's own scope, revisit if a real violation ever slips past them undetected. Turning on `cargo fmt --check` as a new gate also surfaced pre-existing, workspace-wide formatting drift (84 files, accumulated because no CI ever enforced `rustfmt` before); that drift was corrected via a single `cargo fmt --all` pass as part of landing this ADR, since shipping a fmt gate that is red on `main` from its very first run would contradict this ADR's own goal.

## ADR-023 — GitHub Releases-only distribution; no code signing or Marketplace/Open VSX publishing yet

**Status:** Accepted

### Context
EPIC-25 (Distribution & Updates) — T-257/US-124 (distribute the CLI+TUI), T-258/US-125 (package Desktop), T-259/US-126 (publish a compatible VS Code extension package) — remained entirely on the backlog before this decision: no prebuilt binary existed for any OS, no packaged Desktop installer existed, and the VS Code extension was neither bundled into a `.vsix` nor published anywhere (ADR-015 already covers *why* the extension doesn't bundle a `gitsail` binary itself, but said nothing about how the extension's own package reaches a user). §39's "open architecture decisions" list already named "Signing/notarization strategy for releases" as unresolved. The repository owner made an explicit, real decision this session, not deferred to "figure out later": GitSail is used personally first, then released on GitHub for anyone who wants to try it — deliberately **before** a code-signing certificate or a Marketplace/Open VSX publisher account exists, both of which depend on external resources this project does not control.

### Decision
1. **Distribution channel: GitHub Releases only, for all three artifact families (CLI/TUI, Desktop, VS Code extension).** `.github/workflows/release.yml`, triggered by pushing a `vX.Y.Z` tag, builds every artifact and attaches them — plus a `SHA256SUMS.txt` covering all of them — to one GitHub Release per tag via `gh release create` using only the default `GITHUB_TOKEN` (no additional secrets). "Verifiable origin" for v0.1's distribution means exactly this: the artifact, the workflow run that built it, and a checksum a downloader can check it against — not a cryptographic publisher signature.
2. **No code signing or notarization yet, recorded explicitly, not silently omitted.** Windows `.msi`/NSIS installers and CLI/TUI `.exe` binaries are not Authenticode-signed; macOS `.dmg`/`.app` bundles and CLI/TUI binaries are not signed or notarized. This means SmartScreen and Gatekeeper will both show an unrecognized-publisher warning on first run — documented in `docs/architecture/release-process.md`, every release's own notes (`docs/architecture/release-notes-template.md`), and each CLI/TUI archive's bundled `README.txt` (`docs/architecture/release-artifact-readme.txt`), so this is never presented as if it were a normal, unremarkable install experience.
3. **No Marketplace/Open VSX publishing yet.** `apps/vscode`'s `.vsix` is attached to the GitHub Release; "Install from VSIX..." is the extension's official installation path for now. `package.json`'s `publisher: "gitsail"` is recorded as an **unregistered placeholder** — it has never been claimed on the Marketplace — not a confirmed identity, so a future real publish must first confirm or change it rather than assume it is already reserved.
4. **`docs/architecture/release-process.md` is the living checklist for reversing 2 and 3** once a certificate/notarization account or Marketplace/Open VSX publisher token exists: which exact `tauri.conf.json` fields (`bundle.windows.certificateThumbprint`/`signCommand`, `bundle.macOS.signingIdentity`) and which new CI secrets/jobs (a signing step before bundling; a `vsce publish`/`ovsx publish` job after packaging) each transition needs. This ADR does not implement any of that — it records the plan so the next session does not have to rediscover it.

### Consequences
Positive: v0.1 users (starting with the repository owner) can install a real, checksummed CLI/TUI/Desktop/VS Code build without compiling from source, for the first time — EPIC-25's core value — using only free, already-available infrastructure (GitHub Releases, the default `GITHUB_TOKEN`). Every artifact's version is unambiguous (the tag, not the frozen `0.0.0` Cargo/package version ADR-021 already established pre-1.0). Trade-off: every Windows/macOS install triggers an OS security warning until a certificate exists, and the VS Code extension is not discoverable through the Marketplace's own search/install UI — both real, user-facing costs, accepted deliberately for this stage rather than blocking any release on external resources outside this project's control. Signing/notarization and Marketplace/Open VSX publishing remain the same "genuinely open, blocked on external resources" item `docs/manual/roadmap-and-open-decisions.md` already tracked before this ADR — this decision does not resolve that dependency, it only makes explicit what ships in the meantime and exactly what changes once it is resolved.

## ADR-024 — Desktop update checking is check-only, never an auto-installer; the running release tag is embedded at build time

**Status:** Accepted

### Context
T-260/US-127 (EPIC-25 — Distribution & Updates) asks GitSail to let a user "update the app with control, to receive fixes without breaking my integrations" — version/origin, integrity/authenticity per the registered strategy, graceful failure/interruption recovery, and a protocol-compatibility note before any switch. ADR-023 already decided distribution is GitHub-Releases-only with **no code signing/notarization and no signed auto-update channel** for now. An update mechanism that silently downloaded and installed an unsigned binary would both be a real security risk (no way to verify the download came from this project rather than a compromised mirror/MITM) and directly contradict ADR-023's own decision — so this story cannot mean "add an auto-updater." Separately, ADR-021 pins every crate's Cargo version (and `tauri.conf.json`'s own `version`) at `"0.0.0"` through every pre-1.0 milestone, which left a genuine, previously-unaddressed gap this story's own scope calls out: a running Desktop process had no way to know *which release it is*, since neither file ever says anything but `0.0.0`.

### Decision
1. **Desktop's update mechanism is check-only: it asks GitHub's Releases API whether a newer tag exists, and shows the answer — it never downloads, verifies a signature on, or installs anything on the person's behalf.** `gitsail_application::update_check::CheckForUpdate` (a new use case) compares the running build's own release tag against `GET https://api.github.com/repos/rpaggi/gitsail/releases/latest` (via `gitsail-forge`'s new `GitHubReleaseUpdateAdapter`, reusing the same `HttpClient`/`FakeHttpClient` seam T-245 established — no second HTTP client). When GitHub's tag is newer, Desktop shows the version, a link to the Release page (the verifiable origin, exactly as ADR-023's own "origin" already means for a manual download), and a link to that release's `SHA256SUMS.txt` — download and installation stay a fully manual action, identical to what a person would do without GitSail. "Authenticity" is stated honestly: there is no signature verification (consistent with ADR-023), only a checksum a manual downloader can check.
2. **Never an unsolicited network call.** An automatic check (on Desktop launch) only ever runs when the `check_for_updates` preference is on (default: on, always overridable — `PreferencesPort`, T-247's own mechanism) *and* at least 24 hours have passed since the last real attempt (`Preferences::last_update_check_unix`, persisted so the throttle survives a restart). An explicit "check for updates now" click bypasses both — it is user-initiated by definition, not automatic. Neither path ever loops or retries on its own.
3. **Failure is always a distinct, non-crashing state, never a hang or a corrupted install.** Since there is no download/install step, "recovery" is simple by construction: a network failure, a malformed API response, or "no releases published yet" each become one explicit `UpdateCheckOutcome` variant (`CheckFailed`, `NoReleasesPublished`) — the app is never blocked, and the person can retry at any time (the button always works again immediately).
4. **Protocol-compatibility awareness is honest about its own limit.** `docs/architecture/protocol-compatibility.md` already documents that Desktop's IPC has no independently-versioned `schemaVersion` today (frontend and backend ship as one build). There is no way to inspect a *remote* release's `schemaVersion` without downloading it, so this story does not invent one — the update notice does not claim a compatibility check it cannot perform; general compatibility guidance stays in `protocol-compatibility.md`, referenced from the update UI's own documentation rather than duplicated as a fabricated per-release check.
5. **The running release tag is embedded at build time, closing the ADR-021 gap.** `apps/desktop/src-tauri/build.rs` reads the `RELEASE_TAG` environment variable — already exported at the workflow level by `.github/workflows/release.yml` (used since T-258 to stamp the installer's own display version) — and emits it as a compile-time `GITSAIL_APP_VERSION` env var via `cargo:rustc-env`. `version::running_version_tag()` reads it with `env!`, always defined (falling back to the literal `"dev"`, mapped to `None`, for any build the release pipeline did not produce — a local `cargo tauri dev`/`cargo test`). This is a build-time constant, deliberately not a runtime file read or network call: what a binary reports about its own version can never drift after compilation.
6. **CLI, TUI, and the VS Code extension get no update check in this story.** They have no equivalent of `AppState`/a long-lived process settings surface to hang a "check automatically" preference off of the same way Desktop does, and duplicating `gitsail-forge`'s adapter into three more, differently-shaped surfaces was judged out of this story's scope. `docs/manual/troubleshooting.md`'s "Updates" section states this plainly: check https://github.com/rpaggi/gitsail/releases manually for those three.

### Consequences
Positive: a GitSail Desktop user now knows when a newer release exists, with a verifiable origin and a checksum, without GitSail ever needing to be trusted with silent code execution — this is exactly the integrity story ADR-023's own no-signing decision can honestly support today. The ADR-021 "which version am I" gap is closed with a small, targeted build-time mechanism rather than a workaround. Trade-off: this is meaningfully less convenient than a real auto-updater (the person still downloads and runs an installer by hand, and still sees the OS's unsigned-publisher warning ADR-023 already documents) — accepted deliberately, the same way ADR-023 accepted the signing warning, rather than building a false sense of "handled automatically" on top of an unsigned channel. CLI/TUI/VS Code staying without any check is a real, stated scope gap (not a silent one) — a future story can extract `gitsail_application::update_check`'s use case into those surfaces once each has a natural home for the "check automatically" preference and its own throttle state.

# 39. Open architecture decisions

The following remain deliberately unresolved:
- MIT vs Apache-2.0.
- Tokio vs another async strategy in infrastructure.
- Minimum supported Git version.
- Rust MSRV.
- Desktop state-management library.
- Persistence format for GitSail preferences.
- Filesystem watcher library.
- IPC technology if/when CLI JSON is replaced.
- Signing/notarization strategy for releases.
- Submodule architecture and target release.

> **Update (ADR-021, 2026-09-18):** "MIT vs Apache-2.0" and "Rust MSRV" are resolved — Apache-2.0 (`LICENSE`) and `1.97.0` respectively. "Minimum supported Git version" is now recorded as 2.31, derived from the strictest flag `gitsail-git` already uses. The list above is left otherwise unedited as the original v0.1 baseline; see ADR-021 for the resolutions and their rationale, and the backlog's §8 decision table for the up-to-date open/closed status of every item in this list.

> **Update (ADR-023, 2026-09-18):** "Signing/notarization strategy for releases" now has a registered decision — deliberately **not yet** signing/notarizing, in favor of GitHub Releases-only distribution, until a certificate/notarization account exists — rather than remaining an unstated gap. See ADR-023 and `docs/architecture/release-process.md` for the decision, its consequences, and the exact steps that would reverse it once those external resources exist.

> **Update (ADR-024, 2026-09-18):** Desktop's update mechanism (not originally named in this list, but the natural next question once ADR-023 shipped signing-less distribution) is now decided: check-only against GitHub Releases, never an auto-installer, consistent with ADR-023's own no-signing decision. See ADR-024 and `docs/architecture/update-mechanism.md`. CLI/TUI/VS Code remain without any update check.

# 40. Architecture definition of done for v0.1

Before v0.1 is considered architecturally complete:

- Domain compiles without presentation/infrastructure dependencies.
- Application use cases depend on ports, not Git CLI implementation.
- Git CLI adapter passes integration/contract tests.
- Status, log, branches, diff and blame return typed domain models.
- CLI exposes human and versioned JSON outputs.
- Errors have stable categories.
- Windows/Linux/macOS CI passes.
- Repository fixtures cover key edge cases.
- No UI layer executes raw Git commands.
- Architecture decisions are stored with the source repository.

> **Update (ADR-022, 2026-09-18):** "Windows/Linux/macOS CI passes" is now met — `.github/workflows/ci.yml`'s `rust` job runs `cargo fmt`/`clippy`/`test`/`build` on all three OSes. See ADR-022 and `docs/architecture/ci-policy.md` for the full pipeline, which checks are required for merge vs. informative, and its documented assumptions/sandbox-verification limits.

**GitSail — Navigate your Git history.**
