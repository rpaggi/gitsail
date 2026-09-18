//! Maps a [`GitSailError`] category to a stable process exit code (US-038
//! criterion 2: "Falhas distinguem Git ausente, permissão, parsing e
//! operação cancelada").
//!
//! `2` is reserved for CLI usage errors: `clap` already exits with it
//! (and its own message) before any of this ever runs, so it is never
//! produced here. `130` follows the POSIX convention of `128 + SIGINT` for a
//! cancelled command (US-038 criterion 1).

use gitsail_domain::ErrorCode;

pub const EXIT_OK: i32 = 0;
pub const EXIT_GENERIC_FAILURE: i32 = 1;
pub const EXIT_REPOSITORY_NOT_FOUND: i32 = 3;
pub const EXIT_GIT_UNAVAILABLE: i32 = 4;
pub const EXIT_REPOSITORY_STATE: i32 = 5;
pub const EXIT_ACCESS_DENIED: i32 = 6;
pub const EXIT_NETWORK_FAILURE: i32 = 7;
pub const EXIT_TIMEOUT: i32 = 8;
pub const EXIT_CANCELLED: i32 = 130;

pub fn exit_code_for(code: ErrorCode) -> i32 {
    match code {
        ErrorCode::RepositoryNotFound => EXIT_REPOSITORY_NOT_FOUND,
        ErrorCode::GitNotInstalled | ErrorCode::UnsupportedGitVersion => EXIT_GIT_UNAVAILABLE,
        ErrorCode::InvalidRepositoryState
        | ErrorCode::OperationConflict
        | ErrorCode::RepositoryLocked => EXIT_REPOSITORY_STATE,
        ErrorCode::AuthenticationRequired | ErrorCode::PermissionDenied => EXIT_ACCESS_DENIED,
        ErrorCode::NetworkFailure => EXIT_NETWORK_FAILURE,
        ErrorCode::Timeout => EXIT_TIMEOUT,
        ErrorCode::Cancelled => EXIT_CANCELLED,
        ErrorCode::ProcessFailure
        | ErrorCode::ParseFailure
        | ErrorCode::ProtocolMismatch
        | ErrorCode::Internal => EXIT_GENERIC_FAILURE,
        // ErrorCode is #[non_exhaustive]: a category added later falls back
        // to a generic failure rather than failing to compile here.
        _ => EXIT_GENERIC_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_git_absent_permission_parsing_and_cancelled() {
        assert_eq!(exit_code_for(ErrorCode::GitNotInstalled), EXIT_GIT_UNAVAILABLE);
        assert_eq!(exit_code_for(ErrorCode::PermissionDenied), EXIT_ACCESS_DENIED);
        assert_eq!(exit_code_for(ErrorCode::ParseFailure), EXIT_GENERIC_FAILURE);
        assert_eq!(exit_code_for(ErrorCode::Cancelled), EXIT_CANCELLED);
        assert_ne!(
            exit_code_for(ErrorCode::GitNotInstalled),
            exit_code_for(ErrorCode::PermissionDenied)
        );
        assert_ne!(
            exit_code_for(ErrorCode::ParseFailure),
            exit_code_for(ErrorCode::Cancelled)
        );
    }

    #[test]
    fn success_and_usage_exit_codes_are_reserved_and_never_produced_by_this_mapping() {
        for code in ErrorCode::ALL {
            let mapped = exit_code_for(code);
            assert_ne!(mapped, EXIT_OK);
            assert_ne!(mapped, 2, "exit code 2 is reserved for CLI usage errors");
        }
    }
}
