//! Commit entity and related value objects (SAD §8).

use crate::ids::{BranchName, CommitHash, ShortHash};

/// A point in time as reported by Git: seconds since the Unix epoch plus the
/// signer's UTC offset. Kept as raw components (no external date/time
/// dependency) so domain types stay runtime-independent; presentation
/// layers format as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GitTimestamp {
    pub seconds_since_epoch: i64,
    pub utc_offset_minutes: i32,
}

impl GitTimestamp {
    pub const fn new(seconds_since_epoch: i64, utc_offset_minutes: i32) -> Self {
        Self {
            seconds_since_epoch,
            utc_offset_minutes,
        }
    }
}

/// The author or committer of a commit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Signature {
    pub name: String,
    pub email: String,
}

impl Signature {
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
        }
    }
}

/// A ref decoration attached to a commit for display (e.g. `git log
/// --decorate`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Decoration {
    Head,
    Branch(BranchName),
    RemoteBranch { remote: String, branch: BranchName },
    Tag(String),
}

/// A single commit object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: CommitHash,
    pub short_hash: ShortHash,
    pub parents: Vec<CommitHash>,
    pub author: Signature,
    pub committer: Signature,
    pub author_date: GitTimestamp,
    pub commit_date: GitTimestamp,
    pub subject: String,
    pub body: String,
    pub decorations: Vec<Decoration>,
}

impl Commit {
    /// Whether this commit has more than one parent.
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Whether this commit has no parents (a root commit).
    pub fn is_root(&self) -> bool {
        self.parents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commit() -> Commit {
        Commit {
            hash: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            short_hash: ShortHash::new("deadbeef").unwrap(),
            parents: vec![],
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: "root commit".into(),
            body: String::new(),
            decorations: vec![],
        }
    }

    #[test]
    fn root_commit_has_no_parents() {
        assert!(sample_commit().is_root());
        assert!(!sample_commit().is_merge());
    }

    #[test]
    fn merge_commit_has_multiple_parents() {
        let mut commit = sample_commit();
        commit.parents = vec![
            CommitHash::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
            CommitHash::new("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap(),
        ];
        assert!(commit.is_merge());
        assert!(!commit.is_root());
    }
}
