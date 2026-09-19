Prebuilt binaries and installers for this release:

- **CLI + TUI**: `gitsail-<tag>-linux-x86_64.tar.gz`, `gitsail-<tag>-windows-x86_64.zip`, `gitsail-<tag>-macos-aarch64.tar.gz` (Apple Silicon only for now — see "Known gaps" in `docs/architecture/release-process.md`). Requires a separately installed Git ≥ 2.31 on `PATH` — each archive includes a `README.txt` with per-OS install instructions.
- **Desktop**: `.deb` / `.AppImage` (Linux), `.msi` / NSIS `.exe` (Windows), `.dmg` / `.app.zip` (macOS).
- **VS Code extension**: `gitsail-vscode-<tag>.vsix` — install via VS Code's "Extensions: Install from VSIX..." command. Self-contained: it reads Git directly and needs nothing from this release but itself — the only requirement is a separately installed Git ≥ 2.31 on `PATH` (ADR-025, which supersedes ADR-015).

**Verify before you run anything**: download `SHA256SUMS.txt` from this same release and confirm it against the asset(s) you downloaded (`sha256sum -c SHA256SUMS.txt` on Linux/macOS, or `Get-FileHash` on Windows).

**No code signing or notarization yet.** Windows and macOS will show an unsigned-binary warning (SmartScreen / Gatekeeper) when you first run an installer from this release. This is a registered, deliberate decision for now, not an oversight — see ADR-023 and `docs/architecture/release-process.md` for the full rationale and what changes once a certificate/notarization account is available.

**Not yet published to the VS Code Marketplace or Open VSX.** Installing from the attached `.vsix` is the supported path today — see `docs/architecture/release-process.md`.

Full documentation: [`README.md`](https://github.com/rpaggi/gitsail#readme) · [release process](https://github.com/rpaggi/gitsail/blob/main/docs/architecture/release-process.md) · [manuals](https://github.com/rpaggi/gitsail/tree/main/docs/manual).
