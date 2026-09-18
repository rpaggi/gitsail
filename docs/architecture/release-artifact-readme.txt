GitSail — CLI and TUI (this archive)
=====================================

This archive contains two prebuilt binaries built directly from the GitSail
source tree at the release tag recorded in VERSION.txt:

  - gitsail       (or gitsail.exe on Windows)      — the CLI
  - gitsail-tui   (or gitsail-tui.exe on Windows)  — the terminal UI

Neither binary bundles Git itself. GitSail shells out to a real, separately
installed `git` executable (ADR-003) — it does not reimplement Git.

Git prerequisite (minimum version 2.31, per ADR-021)
-----------------------------------------------------

Linux:
  Install Git through your distribution's package manager, then confirm the
  version is 2.31 or newer:
    Debian/Ubuntu:  sudo apt-get install git
    Fedora:         sudo dnf install git
    Arch:           sudo pacman -S git
    git --version

macOS:
  Git ships with the Xcode Command Line Tools:
    xcode-select --install
  or, via Homebrew (often a newer version than Apple's bundled one):
    brew install git
    git --version

Windows:
  Install Git for Windows:
    https://git-scm.com/download/win
  or via winget:
    winget install --id Git.Git -e
  Then confirm the version from a terminal:
    git --version

If `git --version` reports an older version than 2.31, some GitSail
operations may fail or behave unexpectedly — see ADR-021 in
docs/architecture/GitSail_SAD_and_ADRs_v0.1.md for exactly which Git
features that floor depends on.

Running GitSail
----------------

Put this archive's directory on your PATH, or run the binaries directly:
  ./gitsail --help
  ./gitsail-tui

On Windows, the equivalent is `gitsail.exe --help` / `gitsail-tui.exe`.

Verifying this download
-------------------------

This archive is published as a GitHub Release asset. The release also
includes a SHA256SUMS.txt covering every asset in that release (this
archive, the other platforms' archives, the Desktop installers, and the
VS Code .vsix) — compare this file's checksum against the matching line in
SHA256SUMS.txt before relying on it. GitSail does not (yet) code-sign or
notarize its binaries/installers; see
docs/architecture/release-process.md for the registered decision and what
would need to change once a certificate/notarization account exists.

Note on `--version`
---------------------

Per ADR-021 (pre-1.0 versioning policy), the Cargo package version compiled
into these binaries stays "0.0.0" through every pre-1.0 milestone —
`gitsail --version` intentionally prints "gitsail 0.0.0", not this
release's tag. The tag (see VERSION.txt in this archive, and this archive's
own filename) is the real, authoritative version identifier for a release;
the Cargo version is a separate, deliberately frozen number until v1.0.

License: Apache License 2.0 (see LICENSE in this archive).
