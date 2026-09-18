//! Versioned error payload (SAD §14: "Errors use the same envelope
//! concept"; SAD §19).

use serde::{Deserialize, Serialize};

use gitsail_domain::GitSailError;

/// The wire shape of a [`GitSailError`]: stable code, user-safe message, and
/// optional remediation/correlation. Deliberately excludes
/// [`GitSailError::diagnostic`] — raw Git stderr and other diagnostic detail
/// must never automatically become part of a payload a client parses or
/// displays as the primary message (SAD §19, §28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub operation_id: Option<String>,
}

impl From<&GitSailError> for ErrorPayload {
    fn from(err: &GitSailError) -> Self {
        Self {
            code: err.code().as_str().to_string(),
            message: err.message().to_string(),
            remediation: err.remediation().map(str::to_string),
            operation_id: err.operation_id().map(|id| id.as_str().to_string()),
        }
    }
}

impl From<GitSailError> for ErrorPayload {
    fn from(err: GitSailError) -> Self {
        Self::from(&err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{ErrorCode, OperationId};

    #[test]
    fn maps_every_public_field_and_omits_absent_optionals() {
        let err = GitSailError::new(ErrorCode::RepositoryNotFound, "not a Git repository");
        let payload = ErrorPayload::from(&err);

        assert_eq!(payload.code, "repository_not_found");
        assert_eq!(payload.message, "not a Git repository");
        assert_eq!(payload.remediation, None);
        assert_eq!(payload.operation_id, None);

        let json = serde_json::to_value(&payload).unwrap();
        assert!(json.get("remediation").is_none());
        assert!(json.get("operationId").is_none());
    }

    #[test]
    fn maps_remediation_and_operation_id_when_present() {
        let err = GitSailError::new(ErrorCode::Timeout, "git status timed out")
            .with_remediation("retry with a longer timeout")
            .with_operation_id(OperationId::new("op-1"));
        let payload = ErrorPayload::from(&err);

        assert_eq!(
            payload.remediation.as_deref(),
            Some("retry with a longer timeout")
        );
        assert_eq!(payload.operation_id.as_deref(), Some("op-1"));

        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["operationId"], "op-1");
    }

    #[test]
    fn never_carries_the_diagnostic_source() {
        #[derive(Debug)]
        struct Secret;
        impl std::fmt::Display for Secret {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "super-secret-stderr-contents")
            }
        }
        impl std::error::Error for Secret {}

        let err =
            GitSailError::new(ErrorCode::ProcessFailure, "git process failed").with_source(Secret);
        let payload = ErrorPayload::from(&err);
        let json = serde_json::to_string(&payload).unwrap();

        assert!(!json.contains("super-secret-stderr-contents"));
    }
}
