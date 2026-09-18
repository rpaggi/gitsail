<!--
Thanks for contributing to GitSail. Fill in the sections below — see
CONTRIBUTING.md for the full setup/test/review process this template
follows.

Never paste a token, password, credential-bearing URL, or other secret
anywhere in this description, in a commit message, or in a diff. Redact any
command output that might contain one before pasting it here.
-->

## Related User Story / Epic / Task

<!--
e.g. "Implements US-131 / EPIC-26 (T-264)". If this doesn't map to an
existing backlog entry (docs/product/GitSail_Product_Backlog_v1.0.md) or
Takumi task, say "None — <one-line reason>" instead of leaving this blank.
-->

## Summary

<!-- What changed and why, in a few bullet points. Focus on *why*. -->

-

## Reproduction / verification steps

<!--
For a bug fix: the exact steps that reproduced the bug before this change,
with any secret/private content redacted, and confirmation they no longer
reproduce it.
For a new feature: the exact commands used to exercise it manually.
-->

-

## Tests run locally

<!-- Check every command you actually ran, and only those. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo build --workspace`
- [ ] `bash scripts/ci/check-architecture.sh`
- [ ] `apps/desktop`: `npm run test -- --run` and `npm run build`
- [ ] `apps/vscode`: `npx vitest run` and `npx tsc -p ./`
- [ ] Not applicable — this PR only touches documentation/planning files with no code impact

## Architecture / documentation impact

- [ ] This change stays inside its existing layer boundary (Ports & Adapters — domain does not depend on infra/application/presentation; only `gitsail-git` calls the real `git` binary). See `CONTRIBUTING.md` §3.
- [ ] This change adds or revises an ADR (see `CONTRIBUTING.md` §6 — ADR numbers are never reused; a revision edits in place with a dated note; a new decision gets the next unused number), and the SAD's ADR index is updated accordingly.
- [ ] This change updates `docs/architecture/protocol-compatibility.md` (required if it touches `SCHEMA_VERSION` or a component's supported-version list — ADR-016).
- [ ] None of the above apply.

## Checklist

- [ ] I have redacted any token, password, credential-bearing URL, or other secret from this description and from every diff in this PR.
- [ ] New/changed behavior is covered by a test at the layer that actually exercises it.
- [ ] Technical documentation touched by this PR is written in English (product/backlog documents in Portuguese are exempt — see `AGENTS.md`).
