//! Versioned response envelope (SAD §14; ADR-008).
//!
//! Every response crossing the protocol boundary is wrapped in an
//! [`Envelope`]: `schemaVersion` is mandatory and success/error are the same
//! concept tagged by `status`, so a consumer never has to guess which shape
//! it received (US-035 criteria 1-2).

use serde::{Deserialize, Serialize};

use crate::error::ErrorPayload;
use crate::request_id::RequestId;

/// The schema version this crate currently produces (SAD §14; ADR-014;
/// ADR-016; US-039).
///
/// # Compatibility policy
///
/// This is the single number every consumer (the CLI's own `--json`
/// output, `apps/vscode/src/protocol.ts`'s `SUPPORTED_SCHEMA_VERSIONS`, and
/// any future daemon/IPC transport per ADR-012) checks before trusting the
/// shape of `data`/`error`. The full matrix of which component supports
/// which version in which release lives in
/// `docs/architecture/protocol-compatibility.md` — update it whenever this
/// constant changes.
///
/// **Increment `SCHEMA_VERSION` when a change could make an unmodified
/// existing consumer misinterpret bytes it receives**, for example:
/// - Removing or renaming an existing struct field, enum variant, or
///   `#[serde(tag/rename/rename_all)]` value.
/// - Changing an existing field's type or unit (e.g. seconds to
///   milliseconds) without renaming it.
/// - Changing how an enum is tagged (internally/externally/adjacently) or
///   how `Envelope`/`Page` themselves are shaped.
/// - Adding a new variant to an already-shipped DTO enum. Serde's derived
///   `Deserialize` fails closed on an unrecognized tag (none of these enums
///   use `#[serde(other)]`), so a strict Rust consumer on an older
///   `gitsail-protocol` breaks the moment it sees the new variant — this is
///   a breaking change even though it looks additive.
///
/// **Do not increment it** for a genuinely additive change an existing
/// consumer already tolerates:
/// - Adding a new optional struct field (serde ignores unknown JSON keys by
///   default on the way in, and `skip_serializing_if` keeps it out of the
///   wire format on the way out when absent).
/// - Adding an entirely new DTO type, command, or `Output` variant that no
///   existing consumer path decodes yet.
///
/// When genuinely unsure which bucket a change falls into, increment it:
/// this constant is cheap to bump and expensive to get wrong, per SAD
/// §14's "breaking protocol changes require a new schema version."
///
/// `crates/gitsail-protocol/tests/compatibility.rs` pins the exact wire
/// shape of representative DTOs (one covering each `#[serde(...)]` pattern
/// used in `dto.rs`) against checked-in JSON. It exists specifically so
/// that changing a DTO's wire shape fails a test right here, forcing the
/// author to consciously apply this policy — bump `SCHEMA_VERSION` and
/// update the fixture, or confirm the change is additive and only update
/// the fixture — rather than letting an incompatible shape slip out
/// unnoticed.
pub const SCHEMA_VERSION: u32 = 1;

/// A versioned, correlated response: either successful `data`, or an
/// [`ErrorPayload`] — never both, and always tagged by `status` so a
/// consumer can branch on the wire shape alone (ADR-008).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Envelope<T> {
    Ok {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "requestId")]
        request_id: RequestId,
        data: T,
    },
    Error {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "requestId")]
        request_id: RequestId,
        error: ErrorPayload,
    },
}

impl<T> Envelope<T> {
    pub fn ok(request_id: RequestId, data: T) -> Self {
        Self::Ok {
            schema_version: SCHEMA_VERSION,
            request_id,
            data,
        }
    }

    pub fn error(request_id: RequestId, error: ErrorPayload) -> Self {
        Self::Error {
            schema_version: SCHEMA_VERSION,
            request_id,
            error,
        }
    }

    pub fn schema_version(&self) -> u32 {
        match self {
            Self::Ok { schema_version, .. } | Self::Error { schema_version, .. } => *schema_version,
        }
    }

    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::Ok { request_id, .. } | Self::Error { request_id, .. } => request_id,
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Ok { data, .. } => Some(data),
            Self::Error { .. } => None,
        }
    }
}

/// A single page of DTO results plus continuation metadata, mirroring
/// [`gitsail_application::Page`] at the protocol boundary (SAD §14, §25).
/// Kept as its own DTO rather than reusing the application type directly:
/// the application `Page` is generic over domain models, while this one is
/// always over a serializable DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, next_cursor: Option<String>, has_more: bool) -> Self {
        Self {
            items,
            next_cursor,
            has_more,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_envelope_round_trips_through_json_with_the_documented_shape() {
        let envelope = Envelope::ok(RequestId::new("req-1"), "hello".to_string());
        let json = serde_json::to_value(&envelope).unwrap();

        assert_eq!(json["status"], "ok");
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["requestId"], "req-1");
        assert_eq!(json["data"], "hello");

        let round_tripped: Envelope<String> = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, envelope);
    }

    #[test]
    fn error_envelope_carries_the_error_payload_and_no_data_field() {
        let payload = ErrorPayload {
            code: "repository_not_found".to_string(),
            message: "not a Git repository".to_string(),
            remediation: None,
            operation_id: None,
        };
        let envelope: Envelope<String> = Envelope::error(RequestId::new("req-2"), payload.clone());
        let json = serde_json::to_value(&envelope).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(json["error"]["code"], "repository_not_found");
        assert!(json.get("data").is_none());
        assert!(!envelope.is_ok());
    }

    #[test]
    fn page_omits_next_cursor_when_there_is_no_more_data() {
        let page = Page::new(vec![1, 2, 3], None, false);
        let json = serde_json::to_value(&page).unwrap();

        assert!(json.get("nextCursor").is_none());
        assert_eq!(json["hasMore"], false);
    }
}
