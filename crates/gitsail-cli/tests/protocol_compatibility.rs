//! End-to-end producer/consumer contract for the protocol envelope
//! (US-039 DoD: "suíte produtor/consumidor cobre as versões suportadas e a
//! rejeição de schema desconhecido... o produtor real gera um envelope, o
//! consumidor real aceita a versão suportada").
//!
//! Unlike `crates/gitsail-protocol/tests/compatibility.rs` (which feeds
//! hand-written fixtures to the consumer-side helper), this test spawns
//! the real, compiled `gitsail` binary as the producer and feeds its
//! actual stdout straight into `gitsail_protocol::parse_envelope` as the
//! consumer.
//!
//! This used to be described as the same two endpoints VS Code's
//! `cliClient.ts` connected in production. That is no longer true: ADR-025
//! moved the VS Code extension onto `git` directly, so it consumes no
//! envelope at all and that file is gone. The contract this test pins is
//! unaffected — `gitsail --json` is still a real producer with real
//! consumers (any scripted/automated use of the CLI, and any future
//! consumer across a versioned process boundary), and the rejection of an
//! unknown schema still has to hold for them.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_protocol::{parse_envelope, EnvelopeDecodeError, RepositoryStatusDto};

fn temp_repo_dir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("gitsail-cli-protocol-compat-{nanos}-{n}"));
    std::fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn init_repo(dir: &PathBuf) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("LC_ALL", "C")
            .status()
            .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    };
    run(&["init", "--quiet", "--initial-branch=main"]);
    run(&["config", "user.name", "Test User"]);
    run(&["config", "user.email", "test@example.com"]);
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gitsail"))
}

#[test]
fn the_real_cli_producer_output_is_accepted_by_the_real_protocol_consumer() {
    let dir = temp_repo_dir();
    init_repo(&dir);

    let output = Command::new(bin())
        .args(["status", "--repo", dir.to_str().unwrap(), "--json"])
        .output()
        .expect("failed to run gitsail status --json");
    assert!(
        output.status.success(),
        "gitsail status --json failed: {output:?}"
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout must be UTF-8");

    // The real producer's bytes, fed straight into the real consumer-side
    // helper: no hand-written fixture stands in for either side here.
    let envelope = parse_envelope::<RepositoryStatusDto>(stdout.trim()).expect(
        "gitsail-cli's real --json output must be accepted by gitsail_protocol::parse_envelope",
    );
    assert!(envelope.is_ok());
    assert!(
        envelope.data().unwrap().is_clean,
        "a freshly initialized repo has a clean status"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_error_envelope_is_also_accepted_and_carries_no_data() {
    let dir = temp_repo_dir(); // never initialized as a Git repository

    let output = Command::new(bin())
        .args(["status", "--repo", dir.to_str().unwrap(), "--json"])
        .output()
        .expect("failed to run gitsail status --json");
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout must be UTF-8");

    let envelope = parse_envelope::<RepositoryStatusDto>(stdout.trim())
        .expect("a real error envelope must still be a well-formed, supported-version envelope");
    assert!(!envelope.is_ok());
    assert!(envelope.data().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_consumer_rejects_a_hand_crafted_unsupported_schema_version_the_same_way() {
    // This repository's real producer cannot emit an incompatible
    // schemaVersion today (this story deliberately does not introduce one
    // — see envelope.rs's SCHEMA_VERSION policy), so the "unsupported"
    // half of the contract is exercised with a stand-in payload shaped
    // like `docs/architecture/fixtures/protocol-compatibility/unsupported-v2.json`.
    let raw = r#"{"schemaVersion":2,"ok":true,"requestId":"req-1","result":{"isClean":false}}"#;
    let err =
        parse_envelope::<RepositoryStatusDto>(raw).expect_err("schemaVersion 2 must be rejected");
    assert!(matches!(
        err,
        EnvelopeDecodeError::UnsupportedSchemaVersion(2)
    ));
}
