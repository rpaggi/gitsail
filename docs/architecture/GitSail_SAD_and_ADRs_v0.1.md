# GitSail — Software Architecture Document
**Architecture baseline:** v0.1  
**Covers roadmap:** v0.1 → v1.0  
**Status:** Initial architecture  
**Product:** GitSail  
**Tagline:** *Navigate your Git history.*

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

**GitSail — Navigate your Git history.**
