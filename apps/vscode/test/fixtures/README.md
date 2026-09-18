Fake `gitsail-cli` process doubles used only by this package's own tests,
mirroring the pattern `crates/gitsail-cli/tests/fixtures/fake-git` already
uses for the Rust CLI's own integration tests: small scripts that stand in
for the real process so timeout/cancellation/error-shape tests are
deterministic instead of racing a real binary.

Written as plain Node scripts (not shell scripts) and spawned as
`process.execPath <fixture> <args...>` from the tests, rather than as
directly-executable shebang scripts: that keeps them runnable on Windows
too (SAD §35's CI matrix includes Windows), where a `#!/bin/sh` fixture
would not run without a shell.
