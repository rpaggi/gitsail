//! Runtime schema-version acceptance for a consumer parsing an [`Envelope`]
//! it did not produce itself in-process (SAD §14; ADR-016; US-039).
//!
//! Mirrors `apps/vscode/src/protocol.ts`'s `SUPPORTED_SCHEMA_VERSIONS`/
//! `parseEnvelope` on the Rust side: the same two-phase check (read
//! `schemaVersion` first, reject before touching `data`/`error`) applies
//! whenever a Rust process consumes protocol bytes written by a different
//! build of `gitsail-cli` or a future daemon (ADR-012).
//!
//! Today's in-process Rust consumers — the TUI (`gitsail-tui` calls
//! `gitsail-application`/`gitsail-domain` directly and never links
//! `gitsail-protocol` at all) and Desktop (`gitsail-desktop`'s Tauri
//! commands return `gitsail-protocol` DTOs straight to the webview over
//! Tauri's own IPC, not a `gitsail-cli`-produced `Envelope`) — never
//! exercise this module: both are compiled from the same workspace
//! `Cargo.lock` as the `gitsail-protocol` they link, so there is no
//! independently-versioned Rust binary on the other end for them to
//! disagree with. This module exists for the boundary that *does* cross
//! independently-versioned processes today (`gitsail-cli` producing,
//! something else reading its `--json` output) and for the daemon/IPC
//! transport ADR-012 anticipates, where two independently-released Rust
//! binaries could genuinely drift.

use serde::de::DeserializeOwned;
use std::fmt;

use crate::envelope::{Envelope, SCHEMA_VERSION};

/// Schema versions this build of `gitsail-protocol` accepts when consuming
/// an envelope it did not produce itself. Only ever contains
/// [`SCHEMA_VERSION`] plus, temporarily, an immediately-previous version
/// while a migration is in flight — see
/// `docs/architecture/protocol-compatibility.md`.
pub const SUPPORTED_SCHEMA_VERSIONS: &[u32] = &[SCHEMA_VERSION];

/// The envelope could not be trusted as-is: either it was not valid JSON /
/// did not carry a recognizable `schemaVersion`, or it declared a version
/// this build does not speak.
///
/// Deliberately two distinct variants rather than one generic parse error:
/// [`EnvelopeDecodeError::UnsupportedSchemaVersion`] is the case a caller
/// turns into a "please update X" remediation message (US-039 criterion 2),
/// while [`EnvelopeDecodeError::Malformed`] means the bytes were not a
/// well-formed envelope at all regardless of version.
#[derive(Debug)]
pub enum EnvelopeDecodeError {
    /// The payload was not valid JSON, or did not carry a numeric
    /// `schemaVersion` field, or (once that check passed) did not match the
    /// `Envelope<T>` shape for the requested `T`.
    Malformed(serde_json::Error),
    /// `schemaVersion` was present and numeric but not one of
    /// [`SUPPORTED_SCHEMA_VERSIONS`]. Carries the version so a caller can
    /// build a remediation message without re-parsing anything.
    ///
    /// Reached *before* this crate ever attempts to interpret `data` or
    /// `error` as the caller's `T` — an unsupported version is rejected
    /// outright, never partially decoded as if it were valid (US-039
    /// criterion 2).
    UnsupportedSchemaVersion(u32),
}

impl fmt::Display for EnvelopeDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(err) => write!(f, "malformed protocol envelope: {err}"),
            Self::UnsupportedSchemaVersion(version) => write!(
                f,
                "envelope declares schemaVersion {version}, which this build of gitsail-protocol does not understand (supported: {SUPPORTED_SCHEMA_VERSIONS:?}); update the producer or consumer so both speak the same protocol version"
            ),
        }
    }
}

impl std::error::Error for EnvelopeDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(err) => Some(err),
            Self::UnsupportedSchemaVersion(_) => None,
        }
    }
}

/// Parses one `Envelope<T>` from raw JSON bytes this process did not
/// produce itself, rejecting an unsupported `schemaVersion` before ever
/// attempting to decode `data`/`error` as `T` (US-039 criteria 2-3).
///
/// This is a two-phase parse, mirroring `protocol.ts`'s `parseEnvelope`:
/// 1. Parse the bytes as generic JSON and read `schemaVersion` alone.
/// 2. Only if that version is in [`SUPPORTED_SCHEMA_VERSIONS`], decode the
///    full value as `Envelope<T>`.
///
/// A future incompatible schema could reuse the field name `data` for an
/// unrelated shape; step 1 guarantees this function never reaches step 2
/// for such a payload, so it can never coerce or misinterpret it.
pub fn parse_envelope<T: DeserializeOwned>(raw: &str) -> Result<Envelope<T>, EnvelopeDecodeError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(EnvelopeDecodeError::Malformed)?;

    let schema_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            EnvelopeDecodeError::Malformed(<serde_json::Error as serde::de::Error>::custom(
                "missing or non-numeric \"schemaVersion\" field",
            ))
        })?;

    let supported = SUPPORTED_SCHEMA_VERSIONS
        .iter()
        .any(|&version| u64::from(version) == schema_version);
    if !supported {
        // u32::MAX is an obviously-out-of-range sentinel: a schemaVersion
        // this large is unsupported either way, and this path only feeds a
        // human-readable message, never a comparison.
        let reported = u32::try_from(schema_version).unwrap_or(u32::MAX);
        return Err(EnvelopeDecodeError::UnsupportedSchemaVersion(reported));
    }

    serde_json::from_value(value).map_err(EnvelopeDecodeError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_current_schema_version_and_decodes_data() {
        let raw = r#"{"status":"ok","schemaVersion":1,"requestId":"req-1","data":"hello"}"#;
        let envelope: Envelope<String> = parse_envelope(raw).unwrap();
        assert_eq!(envelope.data(), Some(&"hello".to_string()));
    }

    #[test]
    fn rejects_an_unsupported_schema_version_before_touching_data() {
        // `data` deliberately has a shape that would fail to deserialize as
        // `String` if this function ever reached step 2 for it — proving
        // the rejection happens strictly before that attempt.
        let raw = r#"{"status":"ok","schemaVersion":999,"requestId":"req-1","data":{"unexpected":"shape"}}"#;
        let err = parse_envelope::<String>(raw).unwrap_err();
        assert!(matches!(
            err,
            EnvelopeDecodeError::UnsupportedSchemaVersion(999)
        ));
    }

    #[test]
    fn rejects_a_missing_schema_version_as_malformed_not_unsupported() {
        let raw = r#"{"status":"ok","requestId":"req-1","data":"hello"}"#;
        let err = parse_envelope::<String>(raw).unwrap_err();
        assert!(matches!(err, EnvelopeDecodeError::Malformed(_)));
    }

    #[test]
    fn rejects_invalid_json_as_malformed() {
        let err = parse_envelope::<String>("not json").unwrap_err();
        assert!(matches!(err, EnvelopeDecodeError::Malformed(_)));
    }
}
