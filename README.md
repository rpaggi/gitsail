# GitSail

**Navigate your Git history.**

GitSail is an open-source Git client ecosystem with a shared Rust core, CLI, TUI, Desktop application, and Visual Studio Code extension. The repository currently contains the initial workspace, the product and architecture baseline that guides its implementation, and a working `gitsail-cli` binary covering v0.1's read-only queries (repository discovery, status, log, diff, blame, branches, single-commit detail, and line/file history).

## Requirements

- **Git 2.31 or newer** on `PATH` (or pass `--git-path`) — the floor GitSail's Git adapter actually needs; see ADR-021 in the SAD for how that number was derived from the literal arguments the adapter passes.
- **Rust 1.97.0 or newer** (`rustc`/`cargo`) to build `gitsail-cli`, `gitsail-tui`, and the shared core crates. The workspace pins this exact version as its MSRV (`rust-version` in the root `Cargo.toml`).
- **Node.js 18+** and `npm` only if you also want to build/run `apps/desktop` or `apps/vscode`.
- Linux, Windows, or macOS. `apps/desktop` additionally needs the system WebKitGTK/GTK/D-Bus development libraries on Linux (Tauri v2 prerequisite); Windows/macOS need only the Rust toolchain plus, respectively, WebView2 and Xcode Command Line Tools, both already present on current OS installs.

No prebuilt binary has been published yet — no `vX.Y.Z` tag has been pushed
against this repository so far — so build from source below is the only way
to get a working binary today. A release pipeline
(`.github/workflows/release.yml`, ADR-023) exists and is ready: pushing a
`vX.Y.Z` tag builds CLI/TUI archives for Linux/Windows/macOS, Desktop
installers, and a VS Code `.vsix`, checksums them, and publishes them to a
GitHub Release. See [the release process](docs/architecture/release-process.md)
for exactly what that pipeline produces, including the explicit, registered
decision to skip code signing/notarization and Marketplace/Open VSX
publishing for now.

## Installation (from source)

```sh
git clone git@github.com:rpaggi/gitsail.git
cd gitsail
cargo build --release -p gitsail-cli
```

The binary is produced at `target/release/gitsail` (`gitsail.exe` on Windows). Run `./target/release/gitsail --help` to confirm it built correctly, or `cargo run -p gitsail-cli --release -- --help` to build and run in one step.

`cargo check --workspace` verifies the whole Rust workspace (all crates, including the TUI and the Desktop app's Rust backend) without producing release binaries.

## CLI: six real queries

Every example below was executed against a real, disposable Git repository — created for this README with plain `git` commands (two commits on `main`, a third commit on a `feature/greeting` branch, one staged addition, and one unstaged modification) — using the `gitsail` binary built above. Human output is the default; `--json` always emits a single versioned envelope (`schemaVersion`, `requestId`, then `data`) on stdout instead, with nothing else mixed into that stream (SAD §14–15; ADR-014). JSON below is pretty-printed for readability; the CLI itself prints it as one line. Paths shown are the disposable fixture's own absolute path — GitSail always reports paths as `git` resolves them for the repository you actually run against, never a fixed or invented value.

### 1. Discover a repository — `gitsail open`

Resolves `--repo` (default: the current directory) to its identity and current `HEAD` state, whether invoked from the root or a subdirectory.

```
$ gitsail --repo ~/code/gitsail-doc-fixture open
root:   /home/user/code/gitsail-doc-fixture
worktree: /home/user/code/gitsail-doc-fixture
bare:   false
HEAD:   attached to main
branch: main
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json open
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800b7dd78826-0",
    "data": {
        "id": "/home/user/code/gitsail-doc-fixture",
        "rootPath": "/home/user/code/gitsail-doc-fixture",
        "worktreePath": "/home/user/code/gitsail-doc-fixture",
        "isBare": false,
        "headState": { "state": "attached", "branch": "main" },
        "currentBranch": "main"
    }
}
```

### 2. See status — `gitsail status`

Working tree and index status. `A.`/`.M`-style prefixes are index/worktree status side by side (a leading `.` means "no change on that side").

```
$ gitsail --repo ~/code/gitsail-doc-fixture status
HEAD: attached to main
A. README.md
.M src/main.rs
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json status
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800b8f7c4daf-0",
    "data": {
        "branch": "main",
        "headState": { "state": "attached", "branch": "main" },
        "files": [
            { "path": "README.md", "previousPath": null, "changeType": "added", "indexStatus": "added", "worktreeStatus": "unmodified" },
            { "path": "src/main.rs", "previousPath": null, "changeType": "modified", "indexStatus": "unmodified", "worktreeStatus": "modified" }
        ],
        "isClean": false
    }
}
```

### 3. See log/history — `gitsail log`

Paginated commit history (`--limit`, `--cursor`), with optional `--author`/`--grep`/`--path` filters. Human output ends with a `-- more commits available, continue with --cursor N --` hint when `hasMore` is true.

```
$ gitsail --repo ~/code/gitsail-doc-fixture log --limit 2
commit ee913c6f9983d0a6a536084c454dcdd042649a73
Author: Ada Lovelace <ada@example.com>
Date:   1789758569

    Add gitignore

commit 14a5e64447969f2347bd7e42dea75b9c7dd3c967
Author: Ada Lovelace <ada@example.com>
Date:   1789758569

    Add helper stub

-- more commits available, continue with --cursor 2 --
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json log --limit 2
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800b963752ef-0",
    "data": {
        "items": [
            {
                "hash": "ee913c6f9983d0a6a536084c454dcdd042649a73",
                "shortHash": "ee913c6",
                "parents": ["14a5e64447969f2347bd7e42dea75b9c7dd3c967"],
                "author": { "name": "Ada Lovelace", "email": "ada@example.com" },
                "committer": { "name": "Ada Lovelace", "email": "ada@example.com" },
                "authorDate": { "secondsSinceEpoch": 1789758569, "utcOffsetMinutes": -180 },
                "commitDate": { "secondsSinceEpoch": 1789758569, "utcOffsetMinutes": -180 },
                "subject": "Add gitignore",
                "body": "",
                "decorations": [{ "kind": "head" }, { "kind": "branch", "name": "main" }],
                "isMerge": false,
                "isRoot": false
            },
            {
                "hash": "14a5e64447969f2347bd7e42dea75b9c7dd3c967",
                "shortHash": "14a5e64",
                "parents": ["52623dedfe122e3d80c8be3518d4823a00e483b5"],
                "author": { "name": "Ada Lovelace", "email": "ada@example.com" },
                "committer": { "name": "Ada Lovelace", "email": "ada@example.com" },
                "authorDate": { "secondsSinceEpoch": 1789758569, "utcOffsetMinutes": -180 },
                "commitDate": { "secondsSinceEpoch": 1789758569, "utcOffsetMinutes": -180 },
                "subject": "Add helper stub",
                "body": "",
                "decorations": [],
                "isMerge": false,
                "isRoot": false
            }
        ],
        "nextCursor": "2",
        "hasMore": true
    }
}
```

(`gitsail commit <revision>` shows one commit's full detail; `gitsail commit-diff <revision>` shows one commit's diff against its resolved base — not expanded here as separate examples since `log` and `diff` below already demonstrate the same commit/diff shapes.)

### 4. See a diff — `gitsail diff`

With no arguments: unstaged changes (working tree vs. index). `--staged` compares the index against `HEAD`; two revisions compare directly.

```
$ gitsail --repo ~/code/gitsail-doc-fixture diff
diff -- src/main.rs (Modified)
@@ -5,3 +5,4 @@
 fn helper() {
     // TODO: implement
 }
+unstaged change
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json diff
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800b9ce0f83c-0",
    "data": {
        "files": [
            {
                "path": "src/main.rs",
                "previousPath": null,
                "changeType": "modified",
                "isBinary": false,
                "truncated": false,
                "hunks": [
                    {
                        "oldStart": 5, "oldLines": 3, "newStart": 5, "newLines": 4,
                        "lines": [
                            { "origin": "context", "content": "fn helper() {", "hasTrailingNewline": true },
                            { "origin": "context", "content": "    // TODO: implement", "hasTrailingNewline": true },
                            { "origin": "context", "content": "}", "hasTrailingNewline": true },
                            { "origin": "addition", "content": "unstaged change", "hasTrailingNewline": true }
                        ]
                    }
                ]
            }
        ]
    }
}
```

### 5. See blame — `gitsail blame`

Line-by-line attribution, optionally at an older `--revision` and/or restricted to a `--range` (e.g. `10-25`). An uncommitted line is attributed to `local`/"Not Committed Yet", never silently merged into a real commit's authorship.

```
$ gitsail --repo ~/code/gitsail-doc-fixture blame src/main.rs
52623ded (Ada Lovelace             1) fn main() {
52623ded (Ada Lovelace             2)     println!("Hello, GitSail!");
52623ded (Ada Lovelace             3) }
14a5e644 (Ada Lovelace             4)
14a5e644 (Ada Lovelace             5) fn helper() {
14a5e644 (Ada Lovelace             6)     // TODO: implement
14a5e644 (Ada Lovelace             7) }
local (Not Committed Yet        8) unstaged change
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json blame src/main.rs
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800ba3a40e2a-0",
    "data": {
        "file": "src/main.rs",
        "revision": null,
        "lines": [
            { "finalLine": 1, "originalLine": 1, "commit": "52623dedfe122e3d80c8be3518d4823a00e483b5", "author": { "name": "Ada Lovelace", "email": "ada@example.com" }, "timestamp": { "secondsSinceEpoch": 1789758569, "utcOffsetMinutes": -180 }, "content": "fn main() {", "origin": "committed" },
            { "finalLine": 8, "originalLine": 8, "commit": "0000000000000000000000000000000000000000", "author": { "name": "Not Committed Yet", "email": "not.committed.yet" }, "timestamp": { "secondsSinceEpoch": 1789758689, "utcOffsetMinutes": -180 }, "content": "unstaged change", "origin": "local" }
        ]
    }
}
```

(Lines 2–7 elided above for brevity — same shape as lines 1 and 8; the full envelope always includes every requested line.)

### 6. See branches — `gitsail branches`

Local and remote-tracking branches, with upstream/ahead/behind and which one is current. (v0.1's CLI has no dedicated `graph` query yet — commit-graph rendering is a TUI/Desktop presentation concern, SAD §11/ADR-011 — so `branches` is the sixth read query documented here.)

```
$ gitsail --repo ~/code/gitsail-doc-fixture branches
  feature/greeting
* main
```

```
$ gitsail --repo ~/code/gitsail-doc-fixture --json branches
{
    "status": "ok",
    "schemaVersion": 1,
    "requestId": "18d6800baa506cf4-0",
    "data": [
        { "name": "feature/greeting", "kind": { "kind": "local" }, "target": "b2eaee3285424fa2469fe8452caf6f7a39b2358d", "upstream": null, "ahead": 0, "behind": 0, "isCurrent": false },
        { "name": "main", "kind": { "kind": "local" }, "target": "ee913c6f9983d0a6a536084c454dcdd042649a73", "upstream": null, "ahead": 0, "behind": 0, "isCurrent": true }
    ]
}
```

Every subcommand also accepts `--debug` (redacted diagnostics on stderr, never mixed with stdout) and `--timeout <seconds>`. Run `gitsail help <command>` or `gitsail <command> --help` for the full flag list of any command, including `commit`, `commit-diff`, `line-history`, and `show-file`, which are not separately illustrated above.

## Repository layout

- `crates/`: Rust domain, application, Git infrastructure, protocol, CLI, and TUI packages.
- `apps/`: Desktop and Visual Studio Code application boundaries.
- `fixtures/`: Repositories and scenarios used by tests.
- `docs/`: Product and architecture source material.

All new project artifacts are written in English. Historical Portuguese planning documents remain unchanged as source material (see `AGENTS.md`).

## Desktop app

`apps/desktop` is the Tauri v2 + Vue 3 Desktop shell (SAD §17; ADR-006). The Rust backend (`apps/desktop/src-tauri`) is a workspace member and covered by `cargo check --workspace`; it exposes thin Tauri commands (`apps/desktop/src-tauri/src/commands.rs`) that call straight into `gitsail-application`/`gitsail-git` and return `gitsail-protocol` DTOs, mirrored by hand as TypeScript types in `apps/desktop/src/services/dto.ts`.

Local development, from `apps/desktop`:

```sh
npm install
npm run tauri dev   # windowed dev build; needs a Linux display + WebKitGTK, or Windows/macOS
npm run build        # type-checks and builds the frontend only
npm run test          # frontend unit tests (Vitest)
```

On Linux, building the Rust backend also needs the system WebKitGTK/GTK/D-Bus development libraries Tauri links against (see the Tauri v2 Linux prerequisites).

## Architecture and decisions

- The [Software Architecture Document](docs/architecture/GitSail_SAD_and_ADRs_v0.1.md) states the system's goals, style (Ports & Adapters), layer boundaries, and per-area design (protocol, CLI, TUI, Desktop, VS Code, caching, logging, privacy, CI). Its own index at the top lists all 23 Architecture Decision Records with status and a one-line summary — every one is `Accepted` as of this writing.
- Any future split of that document into smaller files must preserve the original text and every ADR's number and content verbatim; see `CONTRIBUTING.md` for the exact rule.

## Documents and assets

- [Product backlog](docs/product/GitSail_Product_Backlog_v1.0.md)
- [Original PRD](docs/product/GitSail_PRD_v0.1-v1.0.md)
- [SAD and 23 ADRs](docs/architecture/GitSail_SAD_and_ADRs_v0.1.md) (ADR-013 onward records decisions made since the original SAD; see the ADR index at the top of that same file)
- [Brand identity — name, tagline, mascot, sail + Git-graph concept](docs/product/brand-identity.md)
- [Original logo](assets/branding/logo_gitsail.png)
- [Original Desktop mockup](assets/mockups/gitsail_gui_mockup.png)
- [Original TUI mockup](assets/mockups/gitsail_tui_mockup.png)
- [Assets inventory — provenance and usage of the files above](assets/README.md)
- [CI policy](docs/architecture/ci-policy.md), [release process](docs/architecture/release-process.md), [performance baseline](docs/architecture/performance-baseline.md), [preferences matrix](docs/architecture/preferences-matrix.md), [protocol compatibility](docs/architecture/protocol-compatibility.md) — operational companions to the ADRs above.
- [CONTRIBUTING.md](CONTRIBUTING.md) — setup, tests, architecture boundaries, and the review/ADR-update process.
- User manuals: [TUI](docs/manual/tui.md), [Desktop](docs/manual/desktop.md), [VS Code extension](docs/manual/vscode.md), [troubleshooting (credentials, configuration, updates, privacy, compatibility)](docs/manual/troubleshooting.md), and [roadmap and open decisions](docs/manual/roadmap-and-open-decisions.md) — what each interface actually does today, and what is deliberately deferred or still undecided.

## Backlog baseline

The backlog defines the planned epics and stories spanning v0.1 through v1.0, with stable IDs, MoSCoW priorities, component ownership, dependencies, acceptance criteria, and definitions of done. It includes source-section traceability, accepted ADR coverage, release gates, and unresolved decisions.

Planning is written in Portuguese for the product owner. New primary technical documentation and code are written in English.

Original source files and PNG assets were copied byte-for-byte from the files supplied by the user. No DOCX files were supplied or reconstructed. The backlog is a planning proposal, not evidence that its features have been implemented.

## Document validation

Validated on 2026-09-17: story and epic counts; unique IDs; required fields; at least three acceptance criteria per story; existing dependency targets; no dependency cycles; no dependency targeting a later milestone; valid local links; byte-for-byte integrity of the original documents and images.

Re-validated on 2026-09-18 (T-264/US-131): every CLI example above was executed against a real, disposable fixture repository with the `gitsail` binary built from this exact source tree, both in human and `--json` form; every link in this README and its ADR cross-references were followed and resolve to an existing file/section (22 ADRs present, ADR-001 through ADR-022, all `Accepted`). ADR-023 (GitHub Releases-only distribution, EPIC-25/T-257–T-259) was added afterward, the same day — see that ADR for what it covers.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build/test setup, architecture boundaries (Ports & Adapters, domain independence), the PR/review process, and how to propose changes to the PRD/SAD/ADRs without breaking their IDs or history.

## License

GitSail is licensed under the [Apache License 2.0](LICENSE). See
`docs/architecture/GitSail_SAD_and_ADRs_v0.1.md` (ADR-021) for why
Apache-2.0 was chosen over MIT, and for the related minimum-Git-version
(2.31), Rust MSRV (1.97.0), repository-identity, and pre-1.0 versioning
decisions recorded alongside it. Trademark/brand-rights availability for the
name "GitSail" and its assets has not been verified — see
`docs/product/brand-identity.md`.
