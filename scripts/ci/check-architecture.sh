#!/usr/bin/env bash
# T-254/US-121 (EPIC-24 — Testing & Quality): lightweight architectural
# fitness functions run in CI (see .github/workflows/ci.yml's
# `architecture-fitness` job).
#
# These are intentionally simple, approximate, grep/text-based checks — not
# a full architectural-conformance tool. See docs/architecture/ci-policy.md
# for what this job is and isn't meant to catch, and for the exceptions
# documented inline below.
set -euo pipefail
cd "$(dirname "$0")/../.."

status=0

echo "=================================================================="
echo "Check 1/2: gitsail-domain must not depend on infra/application/UI"
echo "=================================================================="
# ADR-002 (Ports & Adapters): gitsail-domain is the innermost layer and must
# stay independent of infrastructure, application, and presentation code.
# Any other GitSail workspace crate showing up as a [dependencies] entry of
# gitsail-domain is a violation. External crates (e.g. `url`) are fine and
# intentionally not in this list.
DOMAIN_MANIFEST="crates/gitsail-domain/Cargo.toml"
FORBIDDEN_DOMAIN_DEPS=(
  gitsail-git
  gitsail-application
  gitsail-cli
  gitsail-forge
  gitsail-tui
  gitsail-protocol
  gitsail-test-support
)

if [ ! -f "$DOMAIN_MANIFEST" ]; then
  echo "FAIL: $DOMAIN_MANIFEST not found — has the crate moved? Update this script."
  status=1
else
  violations=0
  for dep in "${FORBIDDEN_DOMAIN_DEPS[@]}"; do
    if grep -Eq "^${dep}[[:space:]]*=" "$DOMAIN_MANIFEST"; then
      echo "VIOLATION: $DOMAIN_MANIFEST declares a dependency on '$dep' (domain must stay independent of infra/application/presentation)."
      violations=1
    fi
  done
  if [ "$violations" -eq 0 ]; then
    echo "OK: no forbidden dependency declared in $DOMAIN_MANIFEST."
  else
    status=1
  fi
fi

echo
echo "=================================================================="
echo "Check 2/2: only designated modules may invoke the 'git' binary"
echo "=================================================================="
# Approximate fitness function (grep-based, per US-121 scope): flags any
# *.rs file outside crates/gitsail-git, or any *.ts file under apps/ outside
# apps/vscode/src/git/process.ts, that spawns a subprocess literally named
# "git". This is intentionally narrow (matches the literal string "git", not
# every subprocess call) because legitimate, unrelated subprocess use exists
# elsewhere — e.g. gitsail-tui/src/browser.rs and
# apps/desktop/src-tauri/src/browser.rs shell out to the OS's URL opener
# (xdg-open/open/cmd), never to git.
#
# The TypeScript half of this check was added by ADR-025 (T-267). Until
# then this script scanned only *.rs — which meant that when the VS Code
# extension started reading Git directly in TypeScript, it did so through a
# gap in the fitness function rather than by permission. A gate that only
# watches one language, while a second language in the same repository does
# the very thing the gate exists to constrain, is not a weaker gate; it is a
# misleading one. Both languages are now scanned by the same rule, and the
# one file allowed to spawn `git` in TypeScript is named below with the same
# weight as the Rust exceptions.
#
# Known, reviewed exceptions (documented here rather than silently
# excluded, so a future reader can see why they're not flagged):
#  - crates/gitsail-test-support/**: a dedicated, dev-dependency-only fixture
#    crate (T-252/US-119) that wires up a *real* GitCliProvider/
#    GitProcessRunner for other crates' integration tests; it is never a
#    normal (non-dev) dependency of anything and ships in no production
#    build.
#  - **/tests/**.rs: integration test binaries (gitsail-tui, gitsail-cli,
#    gitsail-git) that build throwaway fixture repos, several predating
#    gitsail-test-support's adoption (see that crate's module docs for which
#    ones have and haven't migrated).
#  - apps/desktop/src-tauri/src/commands.rs: contains a
#    `#[cfg(test)] mod tests { mod remote_sync_real_git { ... } }` block
#    that shells out to real `git` to build fixture repos for this crate's
#    own inline tests, mirroring gitsail-tui's tests/support/mod.rs (this
#    crate has no public API a separate tests/ binary could reach, so its
#    real-adapter tests live inline — see the comment at that block's
#    definition for the full rationale). A plain grep cannot distinguish
#    "inside #[cfg(test)]" from production code in the same file, so this
#    path is allowlisted explicitly rather than guessed at. Known
#    limitation: if non-test code were ever added to this same file that
#    calls `git` directly, this check would not catch it — documented here
#    and in docs/architecture/ci-policy.md rather than silently accepted.
#  - apps/vscode/src/git/process.ts: the VS Code extension's single Git
#    process boundary (ADR-025). The extension no longer shells out to the
#    `gitsail` CLI — it reads Git directly — and this is the one file in the
#    package permitted to spawn it: every other module under
#    apps/vscode/src/git/ is a pure parser that is handed a string, and
#    nothing outside that directory runs a process at all. This file carries
#    the same obligations crates/gitsail-git/src/runner.rs does (no shell,
#    argv arrays, timeout, cancellation, capped output, redacted stderr);
#    see its module comment. The exception is this narrow *because* the
#    reimplementation it guards is narrow — read-only, seven queries — which
#    is the mitigation ADR-025 rests on.
ALLOWLISTED_FILES=(
  "apps/desktop/src-tauri/src/commands.rs"
  "apps/vscode/src/git/process.ts"
)

# Both scans below run `find` from the repository root, which means they
# also descend into any *other* checkout that happens to live inside the
# working tree — in practice `.claude/worktrees/<branch>/`, the local git
# worktrees an agent harness creates. Those are complete copies of this
# same repository, so every legitimate `crates/gitsail-git` file in them
# appears under a path this script does not recognize as gitsail-git, and
# reports as a violation. They are git-excluded scratch, never part of a
# commit and absent in CI; this check means "this repository's source", so
# it skips them explicitly rather than reporting a copy of allowed code as
# forbidden.
is_allowlisted() {
  local f="$1"
  local allowed
  for allowed in "${ALLOWLISTED_FILES[@]}"; do
    [ "$f" = "$allowed" ] && return 0
  done
  return 1
}

violations=0
while IFS= read -r -d '' file; do
  file="${file#./}"
  case "$file" in
    crates/gitsail-git/*) continue ;;
    crates/gitsail-test-support/*) continue ;;
    */tests/*) continue ;;
  esac
  if is_allowlisted "$file"; then
    continue
  fi
  if grep -Eq '(process::)?Command::new\(\s*"git"\s*\)' "$file"; then
    echo "VIOLATION: $file invokes the 'git' binary directly outside crates/gitsail-git."
    violations=1
  fi
done < <(find . -name '*.rs' -not -path './target/*' -not -path '*/target/*' -not -path './.claude/*' -print0)

# TypeScript half of the same rule (ADR-025). Scans apps/**/*.ts for a
# node:child_process call whose command argument is the literal "git"
# (or "git.exe").
#
# Skipped, for the same reason */tests/*.rs is skipped on the Rust side:
#  - **/test/** and *.test.ts: this repository's suites deliberately drive a
#    *real* git against real temporary repositories rather than a mock (see
#    apps/vscode/test/gitParity.test.ts). Forbidding that would push the
#    tests toward a fake git, which is exactly the thing that lets an
#    adapter's behavior drift unnoticed.
#  - node_modules/out/dist: dependencies and build output, not source.
#
# Known limitation, stated rather than quietly accepted: like its Rust
# counterpart, this is a literal-string grep. A file that puts the command
# in a variable, or builds it by concatenation, would not be flagged. The
# check earns its keep against the accident (someone reaches for `execFile`
# in a new module), not against someone deliberately hiding it.
while IFS= read -r -d '' file; do
  file="${file#./}"
  case "$file" in
    */node_modules/*) continue ;;
    */out/*|*/dist/*) continue ;;
    */test/*|*.test.ts|*.d.ts) continue ;;
  esac
  if is_allowlisted "$file"; then
    continue
  fi
  if grep -Eq '(spawn|spawnSync|exec|execSync|execFile|execFileSync)\s*\(\s*["'"'"']git(\.exe)?["'"'"']' "$file"; then
    echo "VIOLATION: $file spawns the 'git' binary directly outside apps/vscode/src/git/process.ts."
    violations=1
  fi
done < <(find apps -name '*.ts' -not -path '*/.claude/*' -print0 2>/dev/null)

if [ "$violations" -eq 0 ]; then
  echo "OK: no direct 'git' subprocess invocation found in Rust outside crates/gitsail-git, nor in apps/**/*.ts outside apps/vscode/src/git/process.ts (plus the documented exceptions above)."
else
  status=1
fi

echo
if [ "$status" -eq 0 ]; then
  echo "Architecture fitness checks passed."
else
  echo "Architecture fitness checks FAILED."
fi
exit "$status"
