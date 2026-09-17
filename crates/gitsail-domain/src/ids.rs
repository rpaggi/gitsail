//! Validated identifier newtypes shared across domain entities.

use std::fmt;

use crate::error::{ErrorCode, GitSailError};

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A full commit object id (SHA-1 or SHA-256 hex string).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitHash(String);

impl CommitHash {
    pub fn new(hash: impl Into<String>) -> Result<Self, GitSailError> {
        let hash = hash.into();
        if is_hex(&hash) {
            Ok(Self(hash))
        } else {
            Err(GitSailError::new(
                ErrorCode::ParseFailure,
                "commit hash must be a non-empty hexadecimal string",
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derives a short hash by truncating to `len` hex characters.
    pub fn to_short(&self, len: usize) -> ShortHash {
        ShortHash(self.0.chars().take(len).collect())
    }

    /// Whether this is Git's synthetic all-zero hash — e.g. `git blame`'s
    /// "not committed yet" attribution for working-tree content that has no
    /// real commit (US-033) — rather than a hash naming an actual object.
    pub fn is_zero(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }
}

impl fmt::Display for CommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An abbreviated, possibly ambiguous commit id used for display.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShortHash(String);

impl ShortHash {
    pub fn new(hash: impl Into<String>) -> Result<Self, GitSailError> {
        let hash = hash.into();
        if is_hex(&hash) {
            Ok(Self(hash))
        } else {
            Err(GitSailError::new(
                ErrorCode::ParseFailure,
                "short hash must be a non-empty hexadecimal string",
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShortHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A branch name, local (`main`) or remote-tracking (`origin/main`).
///
/// Storage is the plain name as Git reports it; whether it is local or
/// remote is carried separately by [`crate::branch::BranchKind`] rather than
/// encoded into the string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(name: impl Into<String>) -> Result<Self, GitSailError> {
        let name = name.into();
        if name.is_empty() {
            Err(GitSailError::new(
                ErrorCode::ParseFailure,
                "branch name must not be empty",
            ))
        } else {
            Ok(Self(name))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_hash_rejects_non_hex() {
        assert!(CommitHash::new("not-a-hash!").is_err());
        assert!(CommitHash::new("").is_err());
        assert!(CommitHash::new("deadbeef").is_ok());
    }

    #[test]
    fn commit_hash_derives_short_hash() {
        let hash = CommitHash::new("deadbeefcafefeed").unwrap();
        assert_eq!(hash.to_short(8).as_str(), "deadbeef");
    }

    #[test]
    fn commit_hash_detects_the_synthetic_zero_hash() {
        let zero = CommitHash::new("0".repeat(40)).unwrap();
        let real = CommitHash::new("deadbeef").unwrap();
        assert!(zero.is_zero());
        assert!(!real.is_zero());
    }

    #[test]
    fn branch_name_rejects_empty() {
        assert!(BranchName::new("").is_err());
        assert!(BranchName::new("main").is_ok());
    }
}
