fn main() {
    // T-260/US-127: embeds this build's own release tag as a compile-time
    // constant (`crate::version::RUNNING_VERSION_TAG`), closing the
    // ADR-021 gap it leaves open on purpose: Cargo's own `version` (and
    // `tauri.conf.json`'s) stay pinned at `"0.0.0"` through every pre-1.0
    // milestone, so neither can ever tell a running Desktop process "which
    // release am I". `.github/workflows/release.yml` already exports
    // `RELEASE_TAG` (the pushed `vX.Y.Z` tag) as a workflow-level
    // environment variable before its `build-desktop` job runs `npx tauri
    // build` — which spawns this very `cargo build` — so that job's build
    // picks it up here automatically, with no separate wiring needed in
    // the workflow itself (see `docs/architecture/release-process.md`).
    //
    // A local/dev build (no `RELEASE_TAG` in the environment) falls back
    // to the literal `"dev"`, which `version::running_version_tag()`
    // recognizes as "cannot be determined" rather than a real version —
    // never a fabricated `v0.0.0` that could be misread as an actual
    // release.
    let tag = std::env::var("RELEASE_TAG").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=GITSAIL_APP_VERSION={tag}");
    println!("cargo:rerun-if-env-changed=RELEASE_TAG");

    tauri_build::build()
}
