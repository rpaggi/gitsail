// Real extension-host smoke test, using `@vscode/test-electron` to download
// and launch an actual VS Code instance and load this extension inside it —
// the one thing the plain Vitest suite under `test/` cannot exercise (it
// never touches the real `vscode` module, by design — see
// `src/hostTypes.ts`).
//
// Mirrors `apps/desktop`'s own documented gap for its Tauri smoke test
// (T-184: no display/WebKitGTK in this sandbox): this script is expected to
// work on a real developer machine or a GUI-capable CI runner, but this
// project's own sandbox has no installable/launchable VS Code and no
// virtual display server, so it could not be executed here as part of this
// story. See `README.md`'s "Testing" section for the exact limitation.

const path = require("node:path");
const { runTests } = require("@vscode/test-electron");

async function main() {
  const extensionDevelopmentPath = path.resolve(__dirname, "..");
  const extensionTestsPath = path.resolve(__dirname, "suite", "index.js");

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    launchArgs: ["--disable-extensions"],
  });
}

main().catch((err) => {
  console.error("Failed to run VS Code extension tests:", err);
  process.exit(1);
});
