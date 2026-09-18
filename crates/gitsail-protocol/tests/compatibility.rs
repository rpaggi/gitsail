//! Cross-language compatibility contract (US-039 criteria 2-3).
//!
//! Reads the fixtures under
//! `docs/architecture/fixtures/protocol-compatibility/`, shared verbatim
//! with `apps/vscode/test/protocolCompatibility.test.ts` (see that
//! directory's `README.md`), and asserts this crate's own consumer-side
//! helper (`gitsail_protocol::parse_envelope`) accepts the supported
//! version and rejects the hypothetical unsupported one — the Rust half of
//! the same contract the TypeScript client already enforces.

use std::path::{Path, PathBuf};

use gitsail_protocol::{parse_envelope, EnvelopeDecodeError, RepositoryStatusDto};

fn fixture(name: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/architecture/fixtures/protocol-compatibility")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path:?}: {e}"))
}

#[test]
fn accepts_the_supported_schema_version_ok_fixture_and_decodes_its_data() {
    let raw = fixture("ok-v1.json");
    let envelope = parse_envelope::<RepositoryStatusDto>(&raw).expect("ok-v1.json must be accepted");

    assert!(envelope.is_ok());
    let data = envelope.data().expect("ok envelope must carry data");
    assert_eq!(data.branch.as_deref(), Some("main"));
    assert!(!data.is_clean);
    assert_eq!(data.files.len(), 2);
}

#[test]
fn accepts_the_supported_schema_version_error_fixture() {
    let raw = fixture("error-v1.json");
    let envelope = parse_envelope::<RepositoryStatusDto>(&raw).expect("error-v1.json must be accepted");

    assert!(!envelope.is_ok());
    assert!(envelope.data().is_none());
}

#[test]
fn rejects_the_hypothetical_unsupported_schema_version_before_touching_its_payload() {
    let raw = fixture("unsupported-v2.json");

    // `RepositoryStatusDto` cannot be built from `unsupported-v2.json`'s
    // `result` field even if this were reachable (there is no `data` key
    // at all under the hypothetical v2 shape) — so a Malformed error here
    // instead of UnsupportedSchemaVersion would mean the schema check was
    // skipped and decoding was attempted anyway.
    let err = parse_envelope::<RepositoryStatusDto>(&raw).expect_err("schemaVersion 2 must be rejected");

    match err {
        EnvelopeDecodeError::UnsupportedSchemaVersion(version) => assert_eq!(version, 2),
        EnvelopeDecodeError::Malformed(_) => {
            panic!("must reject on the unsupported schemaVersion, not fall through to a decode attempt")
        }
    }
}
