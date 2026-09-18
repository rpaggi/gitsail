#!/usr/bin/env node
"use strict";

// A fake `gitsail-cli` used only by this package's own tests (see
// `test/fixtures/README.md`). Spawned as
// `<node> test/fixtures/fake-cli.js <mode> [...]`, where `<mode>` is the
// first argument, so `GitSailCliClient` tests can exercise every
// envelope/process outcome deterministically instead of depending on a
// real repository or a real `gitsail` build.

const args = process.argv.slice(2);
const mode = args[0];

function writeEnvelope(envelope, exitCode) {
  process.stdout.write(`${JSON.stringify(envelope)}\n`);
  process.exit(exitCode);
}

switch (mode) {
  case "ok":
    writeEnvelope(
      {
        status: "ok",
        schemaVersion: 1,
        requestId: "req-fixture",
        data: {
          id: "repo-fixture",
          rootPath: "/tmp/fixture-repo",
          worktreePath: "/tmp/fixture-repo",
          isBare: false,
          headState: { state: "attached", branch: "main" },
          currentBranch: "main",
        },
      },
      0,
    );
    break;

  case "repository-not-found":
    writeEnvelope(
      {
        status: "error",
        schemaVersion: 1,
        requestId: "req-fixture",
        error: { code: "repository_not_found", message: "not a Git repository" },
      },
      3,
    );
    break;

  case "git-not-installed":
    writeEnvelope(
      {
        status: "error",
        schemaVersion: 1,
        requestId: "req-fixture",
        error: { code: "git_not_installed", message: "git executable not found" },
      },
      4,
    );
    break;

  case "bad-schema":
    writeEnvelope(
      { status: "ok", schemaVersion: 999, requestId: "req-fixture", data: {} },
      0,
    );
    break;

  case "malformed":
    process.stdout.write("this is not json\n");
    process.exit(1);
    break;

  case "no-output":
    process.stderr.write("boom: fixture crashed before printing anything\n");
    process.exit(1);
    break;

  case "hang":
    // Never exits on its own; relies on the caller killing this process
    // (a timeout or a cancellation). `setInterval` keeps the event loop —
    // and therefore the process — alive without busy-waiting.
    setInterval(() => {}, 1000);
    break;

  case "--version":
    process.stdout.write(`gitsail ${args[1] ?? "0.0.0"}\n`);
    process.exit(0);
    break;

  case "unrecognized-version":
    process.stdout.write("definitely not a version string\n");
    process.exit(0);
    break;

  default:
    process.stderr.write(`fake-cli: unknown mode "${mode}"\n`);
    process.exit(2);
}
