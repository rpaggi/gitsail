# GitSail Agent Instructions

`AGENTS.md` is the canonical operational context for every supported agent harness.

## Language

All new project artifacts must be written in English: source code, identifiers, comments, tests, UI text, error messages, technical documentation, and automation output. Historical Portuguese planning documents are source material and must not be translated unless a task explicitly requests it.

## Knowledge base conventions

- The project wiki stores business rules only. Any other kind of document (architecture, product planning, guides, ADRs, contribution process, etc.) belongs under `docs/`, not the wiki.
- Always consult the knowledge base before starting any task.

## Skills

The canonical installed skills live under `.agents/skills/`. They are imported from the Takumi skill catalog's `core` tier. Consult the relevant `SKILL.md` before planning or implementing work in its domain. Do not edit imported skill files locally; refresh them from the catalog when the project adopts the Takumi Skills CLI and lockfile. The installed skills are not enumerated here: the directory tree is the current source of truth.

## Architecture and workflow

- Read the applicable document under `docs/architecture/` and `docs/product/` before changing product behavior.
- Preserve the accepted architecture: Rust shared core, Ports & Adapters boundaries, and Git CLI as the initial provider.
- Keep dependency direction inward. Domain code must not depend on infrastructure or presentation code.
- Treat `.agents/skills/` as the installed skill source and read the applicable `SKILL.md` before acting in its domain.
- Update this file and the relevant documentation whenever a command, architecture decision, or operational convention changes.

## Quality

- Run the documented verification relevant to the change before reporting completion.
- Do not expose credentials, internal hosts, or private network details in project artifacts.
