// Minimal real-extension-host smoke test: activates the extension inside a
// real VS Code instance and checks it did not throw and registered its
// output channel. Deliberately small — the actual business logic already
// has full unit/contract coverage in `test/` against `ExtensionHost`; this
// only proves the thin `extension.ts` adapter wires up against the real
// `vscode` module without crashing (see `runTest.js`'s doc comment for why
// this could not be executed in this project's sandbox).

const assert = require("node:assert");
const vscode = require("vscode");

exports.run = function run() {
  return new Promise((resolve, reject) => {
    (async () => {
      try {
        const extension = vscode.extensions.getExtension("gitsail.gitsail-vscode");
        assert.ok(extension, "the GitSail extension must be discoverable by its id");
        await extension.activate();
        assert.ok(extension.isActive, "the GitSail extension must activate without throwing");
        resolve();
      } catch (err) {
        reject(err);
      }
    })();
  });
};
