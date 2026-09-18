# GitSail

**Navigate your Git history.**

GitSail is an open-source Git client ecosystem with a shared Rust core, CLI, TUI, Desktop application, and Visual Studio Code extension. The repository currently contains the initial workspace and the product and architecture baseline that guides its implementation.

## Repository layout

- `crates/`: Rust domain, application, Git infrastructure, protocol, CLI, and TUI packages.
- `apps/`: Desktop and Visual Studio Code application boundaries.
- `fixtures/`: Repositories and scenarios used by tests.
- `docs/`: Product and architecture source material.

Run `cargo check --workspace` to verify the Rust workspace.

All new project artifacts are written in English. Historical Portuguese planning documents remain unchanged as source material.

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

## Documents and assets

- [Product backlog](docs/product/GitSail_Product_Backlog_v1.0.md)
- [Original PRD](docs/product/GitSail_PRD_v0.1-v1.0.md)
- [Original SAD and 12 ADRs](docs/architecture/GitSail_SAD_and_ADRs_v0.1.md)
- [Original logo](assets/branding/logo_gitsail.png)
- [Original Desktop mockup](assets/mockups/gitsail_gui_mockup.png)
- [Original TUI mockup](assets/mockups/gitsail_tui_mockup.png)

## Backlog baseline

The backlog defines the planned epics and stories spanning v0.1 through v1.0, with stable IDs, MoSCoW priorities, component ownership, dependencies, acceptance criteria, and definitions of done. It includes source-section traceability, accepted ADR coverage, release gates, and unresolved decisions.

Planning is written in Portuguese for the product owner. New primary technical documentation and code are written in English.

Original source files and PNG assets were copied byte-for-byte from the files supplied by the user. No DOCX files were supplied or reconstructed. The backlog is a planning proposal, not evidence that its features have been implemented.

## Document validation

Validated on 2026-09-17: story and epic counts; unique IDs; required fields; at least three acceptance criteria per story; existing dependency targets; no dependency cycles; no dependency targeting a later milestone; valid local links; byte-for-byte integrity of the original documents and images. Product tests and builds are not applicable to this documentation-only delivery.
