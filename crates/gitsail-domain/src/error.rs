//! Domain error model (SAD §19).

use std::error::Error as StdError;
use std::fmt;

/// Stable, machine-checkable error category.
///
/// `ErrorCode` is part of the domain contract: presentation and protocol
/// layers match on it to decide UX, so variants are additive and are never
/// repurposed once released.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    RepositoryNotFound,
    GitNotInstalled,
    UnsupportedGitVersion,
    InvalidRepositoryState,
    OperationConflict,
    AuthenticationRequired,
    PermissionDenied,
    NetworkFailure,
    ProcessFailure,
    ParseFailure,
    Timeout,
    Cancelled,
    ProtocolMismatch,
    Internal,
    /// Another Git process already holds a lock GitSail's own mutation
    /// needs (typically `.git/index.lock`, but any Git lock file matches
    /// the same shape) — SAD §26; T-227/US-116 criterion 1: a mutation that
    /// loses this race must give a clear, actionable error instead of
    /// hanging or corrupting repository state. GitSail never waits for or
    /// removes another process's lock file itself.
    RepositoryLocked,
}

impl ErrorCode {
    /// All categories defined by SAD §19, in document order.
    pub const ALL: [ErrorCode; 15] = [
        ErrorCode::RepositoryNotFound,
        ErrorCode::GitNotInstalled,
        ErrorCode::UnsupportedGitVersion,
        ErrorCode::InvalidRepositoryState,
        ErrorCode::OperationConflict,
        ErrorCode::AuthenticationRequired,
        ErrorCode::PermissionDenied,
        ErrorCode::NetworkFailure,
        ErrorCode::ProcessFailure,
        ErrorCode::ParseFailure,
        ErrorCode::Timeout,
        ErrorCode::Cancelled,
        ErrorCode::ProtocolMismatch,
        ErrorCode::Internal,
        ErrorCode::RepositoryLocked,
    ];

    /// Stable machine-readable identifier, suitable for protocol payloads
    /// and log fields. Never changes for a given variant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepositoryNotFound => "repository_not_found",
            Self::GitNotInstalled => "git_not_installed",
            Self::UnsupportedGitVersion => "unsupported_git_version",
            Self::InvalidRepositoryState => "invalid_repository_state",
            Self::OperationConflict => "operation_conflict",
            Self::AuthenticationRequired => "authentication_required",
            Self::PermissionDenied => "permission_denied",
            Self::NetworkFailure => "network_failure",
            Self::ProcessFailure => "process_failure",
            Self::ParseFailure => "parse_failure",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::Internal => "internal",
            Self::RepositoryLocked => "repository_locked",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Correlates an error with the operation that produced it, across logs,
/// diagnostics and user reports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationId(String);

impl OperationId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A GitSail domain error.
///
/// `message` must always be safe to show a user. Diagnostic detail such as
/// raw Git stderr belongs in `source`: callers may log it, but it must not
/// become the primary UX message by default (SAD §19, §28).
#[derive(Debug)]
pub struct GitSailError {
    code: ErrorCode,
    message: String,
    remediation: Option<String>,
    operation_id: Option<OperationId>,
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl GitSailError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            remediation: None,
            operation_id: None,
            source: None,
        }
    }

    #[must_use]
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }

    #[must_use]
    pub fn with_operation_id(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn remediation(&self) -> Option<&str> {
        self.remediation.as_deref()
    }

    pub fn operation_id(&self) -> Option<&OperationId> {
        self.operation_id.as_ref()
    }

    /// The diagnostic cause, when one was attached. Not part of the
    /// user-safe message; callers decide whether and how to surface it.
    pub fn diagnostic(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        self.source.as_deref()
    }
}

impl fmt::Display for GitSailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl StdError for GitSailError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn StdError + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_have_stable_distinct_identifiers() {
        let mut seen = std::collections::HashSet::new();
        for code in ErrorCode::ALL {
            assert!(seen.insert(code.as_str()), "duplicate error code string");
        }
        assert_eq!(ErrorCode::ALL.len(), seen.len());
    }

    #[test]
    fn display_never_includes_diagnostic_source_by_default() {
        #[derive(Debug)]
        struct Secret;
        impl fmt::Display for Secret {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "super-secret-stderr-contents")
            }
        }
        impl StdError for Secret {}

        let err = GitSailError::new(ErrorCode::ProcessFailure, "git process failed")
            .with_source(Secret);

        let rendered = err.to_string();
        assert!(rendered.contains("git process failed"));
        assert!(!rendered.contains("super-secret-stderr-contents"));
        assert!(err.diagnostic().is_some());
    }

    #[test]
    fn builder_methods_populate_optional_fields() {
        let err = GitSailError::new(ErrorCode::Timeout, "git status timed out")
            .with_remediation("retry with a longer timeout")
            .with_operation_id(OperationId::new("op-1"));

        assert_eq!(err.code(), ErrorCode::Timeout);
        assert_eq!(err.remediation(), Some("retry with a longer timeout"));
        assert_eq!(err.operation_id().map(OperationId::as_str), Some("op-1"));
    }
}
