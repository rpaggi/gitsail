//! Versioned response envelope (SAD §14; ADR-008).
//!
//! Every response crossing the protocol boundary is wrapped in an
//! [`Envelope`]: `schemaVersion` is mandatory and success/error are the same
//! concept tagged by `status`, so a consumer never has to guess which shape
//! it received (US-035 criteria 1-2).

use serde::{Deserialize, Serialize};

use crate::error::ErrorPayload;
use crate::request_id::RequestId;

/// The schema version this crate currently produces. A breaking change to
/// any DTO or envelope shape must increment this (SAD §14, US-039).
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
            Self::Ok { schema_version, .. } | Self::Error { schema_version, .. } => {
                *schema_version
            }
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
