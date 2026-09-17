//! Versioned data-transfer objects mapped explicitly from domain models
//! (SAD §14; ADR-008; US-035).
//!
//! Every DTO here is built through a `From<&domain type>` impl rather than
//! deriving `Serialize` on the domain type itself: the wire shape is this
//! crate's own contract, free to diverge from internal field layout, and a
//! domain-only change (renaming a field, adding a variant) can never
//! silently change the protocol (SAD §14: "Domain structs are not
//! serialized directly by default").

use std::path::Path;

use serde::{Deserialize, Serialize};

use gitsail_domain::{
    Blame, BlameLine, BlameOrigin, Branch, BranchKind, ChangeType, Commit, Decoration, Diff,
    DiffHunk, DiffLine, DiffLineOrigin, FileChange, FileDiff, FileStatusCode, GitTimestamp,
    HeadState, Repository, RepositoryStatus, Signature,
};

/// Converts a filesystem path to its wire representation.
///
/// Uses [`Path::to_string_lossy`], the same lossy conversion
/// [`gitsail_domain::RepositoryId`] already applies for its own display
/// identity: a path containing non-UTF-8 bytes (possible, if rare, on Unix)
/// has those bytes substituted rather than rejected. This is a known,
/// explicitly documented limitation of the current protocol version (SAD
/// §14) rather than a silent one: a future schema version may need a
/// lossless representation for non-UTF-8 paths.
fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------
// Repository / HEAD state.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HeadStateDto {
    Attached { branch: String },
    Detached { commit: String },
    Unborn,
}

impl From<&HeadState> for HeadStateDto {
    fn from(state: &HeadState) -> Self {
        match state {
            HeadState::Attached { branch } => Self::Attached {
                branch: branch.as_str().to_string(),
            },
            HeadState::Detached { commit } => Self::Detached {
                commit: commit.as_str().to_string(),
            },
            HeadState::Unborn => Self::Unborn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDto {
    pub id: String,
    pub root_path: String,
    pub worktree_path: Option<String>,
    pub is_bare: bool,
    pub head_state: HeadStateDto,
    pub current_branch: Option<String>,
}

impl From<&Repository> for RepositoryDto {
    fn from(repo: &Repository) -> Self {
        Self {
            id: repo.id.as_str().to_string(),
            root_path: path_to_string(&repo.root_path),
            worktree_path: repo.worktree_path.as_deref().map(path_to_string),
            is_bare: repo.is_bare,
            head_state: HeadStateDto::from(&repo.head_state),
            current_branch: repo.current_branch.as_ref().map(|b| b.as_str().to_string()),
        }
    }
}

// ---------------------------------------------------------------------
// Status.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeTypeDto {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
    Ignored,
}

impl From<ChangeType> for ChangeTypeDto {
    fn from(value: ChangeType) -> Self {
        match value {
            ChangeType::Added => Self::Added,
            ChangeType::Modified => Self::Modified,
            ChangeType::Deleted => Self::Deleted,
            ChangeType::Renamed => Self::Renamed,
            ChangeType::Copied => Self::Copied,
            ChangeType::TypeChanged => Self::TypeChanged,
            ChangeType::Unmerged => Self::Unmerged,
            ChangeType::Untracked => Self::Untracked,
            ChangeType::Ignored => Self::Ignored,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatusCodeDto {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    UpdatedButUnmerged,
    Untracked,
    Ignored,
}

impl From<FileStatusCode> for FileStatusCodeDto {
    fn from(value: FileStatusCode) -> Self {
        match value {
            FileStatusCode::Unmodified => Self::Unmodified,
            FileStatusCode::Modified => Self::Modified,
            FileStatusCode::Added => Self::Added,
            FileStatusCode::Deleted => Self::Deleted,
            FileStatusCode::Renamed => Self::Renamed,
            FileStatusCode::Copied => Self::Copied,
            FileStatusCode::UpdatedButUnmerged => Self::UpdatedButUnmerged,
            FileStatusCode::Untracked => Self::Untracked,
            FileStatusCode::Ignored => Self::Ignored,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeDto {
    pub path: String,
    pub previous_path: Option<String>,
    pub change_type: ChangeTypeDto,
    pub index_status: FileStatusCodeDto,
    pub worktree_status: FileStatusCodeDto,
}

impl From<&FileChange> for FileChangeDto {
    fn from(change: &FileChange) -> Self {
        Self {
            path: path_to_string(&change.path),
            previous_path: change.previous_path.as_deref().map(path_to_string),
            change_type: change.change_type.into(),
            index_status: change.index_status.into(),
            worktree_status: change.worktree_status.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatusDto {
    pub branch: Option<String>,
    pub head_state: HeadStateDto,
    pub files: Vec<FileChangeDto>,
    pub is_clean: bool,
}

impl From<&RepositoryStatus> for RepositoryStatusDto {
    fn from(status: &RepositoryStatus) -> Self {
        Self {
            branch: status.branch.as_ref().map(|b| b.as_str().to_string()),
            head_state: HeadStateDto::from(&status.head_state),
            files: status.files.iter().map(FileChangeDto::from).collect(),
            is_clean: status.is_clean(),
        }
    }
}

// ---------------------------------------------------------------------
// Commits.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTimestampDto {
    pub seconds_since_epoch: i64,
    pub utc_offset_minutes: i32,
}

impl From<GitTimestamp> for GitTimestampDto {
    fn from(ts: GitTimestamp) -> Self {
        Self {
            seconds_since_epoch: ts.seconds_since_epoch,
            utc_offset_minutes: ts.utc_offset_minutes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureDto {
    pub name: String,
    pub email: String,
}

impl From<&Signature> for SignatureDto {
    fn from(sig: &Signature) -> Self {
        Self {
            name: sig.name.clone(),
            email: sig.email.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DecorationDto {
    Head,
    Branch { name: String },
    RemoteBranch { remote: String, branch: String },
    Tag { name: String },
}

impl From<&Decoration> for DecorationDto {
    fn from(decoration: &Decoration) -> Self {
        match decoration {
            Decoration::Head => Self::Head,
            Decoration::Branch(name) => Self::Branch {
                name: name.as_str().to_string(),
            },
            Decoration::RemoteBranch { remote, branch } => Self::RemoteBranch {
                remote: remote.clone(),
                branch: branch.as_str().to_string(),
            },
            Decoration::Tag(name) => Self::Tag { name: name.clone() },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDto {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: SignatureDto,
    pub committer: SignatureDto,
    pub author_date: GitTimestampDto,
    pub commit_date: GitTimestampDto,
    pub subject: String,
    pub body: String,
    pub decorations: Vec<DecorationDto>,
    pub is_merge: bool,
    pub is_root: bool,
}

impl From<&Commit> for CommitDto {
    fn from(commit: &Commit) -> Self {
        Self {
            hash: commit.hash.as_str().to_string(),
            short_hash: commit.short_hash.as_str().to_string(),
            parents: commit.parents.iter().map(|p| p.as_str().to_string()).collect(),
            author: SignatureDto::from(&commit.author),
            committer: SignatureDto::from(&commit.committer),
            author_date: commit.author_date.into(),
            commit_date: commit.commit_date.into(),
            subject: commit.subject.clone(),
            body: commit.body.clone(),
            decorations: commit.decorations.iter().map(DecorationDto::from).collect(),
            is_merge: commit.is_merge(),
            is_root: commit.is_root(),
        }
    }
}

// ---------------------------------------------------------------------
// Branches.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BranchKindDto {
    Local,
    Remote { remote: String },
}

impl From<&BranchKind> for BranchKindDto {
    fn from(kind: &BranchKind) -> Self {
        match kind {
            BranchKind::Local => Self::Local,
            BranchKind::Remote { remote } => Self::Remote {
                remote: remote.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDto {
    pub name: String,
    pub kind: BranchKindDto,
    pub target: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub is_current: bool,
}

impl From<&Branch> for BranchDto {
    fn from(branch: &Branch) -> Self {
        Self {
            name: branch.name.as_str().to_string(),
            kind: BranchKindDto::from(&branch.kind),
            target: branch.target.as_str().to_string(),
            upstream: branch.upstream.as_ref().map(|u| u.as_str().to_string()),
            ahead: branch.ahead,
            behind: branch.behind,
            is_current: branch.is_current,
        }
    }
}

// ---------------------------------------------------------------------
// Diffs.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineOriginDto {
    Context,
    Addition,
    Deletion,
}

impl From<DiffLineOrigin> for DiffLineOriginDto {
    fn from(origin: DiffLineOrigin) -> Self {
        match origin {
            DiffLineOrigin::Context => Self::Context,
            DiffLineOrigin::Addition => Self::Addition,
            DiffLineOrigin::Deletion => Self::Deletion,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLineDto {
    pub origin: DiffLineOriginDto,
    pub content: String,
    pub has_trailing_newline: bool,
}

impl From<&DiffLine> for DiffLineDto {
    fn from(line: &DiffLine) -> Self {
        Self {
            origin: line.origin.into(),
            content: line.content.clone(),
            has_trailing_newline: line.has_trailing_newline,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunkDto {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineDto>,
}

impl From<&DiffHunk> for DiffHunkDto {
    fn from(hunk: &DiffHunk) -> Self {
        Self {
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            lines: hunk.lines.iter().map(DiffLineDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffDto {
    pub path: String,
    pub previous_path: Option<String>,
    pub change_type: ChangeTypeDto,
    pub is_binary: bool,
    pub truncated: bool,
    pub hunks: Vec<DiffHunkDto>,
}

impl From<&FileDiff> for FileDiffDto {
    fn from(file: &FileDiff) -> Self {
        Self {
            path: path_to_string(&file.path),
            previous_path: file.previous_path.as_deref().map(path_to_string),
            change_type: file.change_type.into(),
            is_binary: file.is_binary,
            truncated: file.truncated,
            hunks: file.hunks.iter().map(DiffHunkDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffDto {
    pub files: Vec<FileDiffDto>,
}

impl From<&Diff> for DiffDto {
    fn from(diff: &Diff) -> Self {
        Self {
            files: diff.files.iter().map(FileDiffDto::from).collect(),
        }
    }
}

// ---------------------------------------------------------------------
// Blame.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlameOriginDto {
    Committed,
    Local,
}

impl From<BlameOrigin> for BlameOriginDto {
    fn from(origin: BlameOrigin) -> Self {
        match origin {
            BlameOrigin::Committed => Self::Committed,
            BlameOrigin::Local => Self::Local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLineDto {
    pub final_line: u32,
    pub original_line: u32,
    pub commit: String,
    pub author: SignatureDto,
    pub timestamp: GitTimestampDto,
    pub content: String,
    pub origin: BlameOriginDto,
}

impl From<&BlameLine> for BlameLineDto {
    fn from(line: &BlameLine) -> Self {
        Self {
            final_line: line.final_line,
            original_line: line.original_line,
            commit: line.commit.as_str().to_string(),
            author: SignatureDto::from(&line.author),
            timestamp: line.timestamp.into(),
            content: line.content.clone(),
            origin: line.origin.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameDto {
    pub file: String,
    pub revision: Option<String>,
    pub lines: Vec<BlameLineDto>,
}

impl From<&Blame> for BlameDto {
    fn from(blame: &Blame) -> Self {
        Self {
            file: path_to_string(&blame.file),
            revision: blame.revision.as_ref().map(|r| r.as_str().to_string()),
            lines: blame.lines.iter().map(BlameLineDto::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        BranchName, CommitHash, GitTimestamp, RepositoryId, ShortHash,
    };
    use std::path::PathBuf;

    #[test]
    fn repository_dto_never_serializes_a_debug_field_and_maps_head_state() {
        let repo = Repository {
            id: RepositoryId::from_canonical_root(Path::new("/repo")),
            root_path: PathBuf::from("/repo"),
            worktree_path: Some(PathBuf::from("/repo")),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        };

        let dto = RepositoryDto::from(&repo);
        let json = serde_json::to_value(&dto).unwrap();

        assert_eq!(json["rootPath"], "/repo");
        assert_eq!(json["headState"]["state"], "attached");
        assert_eq!(json["headState"]["branch"], "main");
        // The DTO's own field set is exhaustively asserted, so an
        // accidental extra (internal) field would fail this comparison.
        let keys: std::collections::HashSet<String> =
            json.as_object().unwrap().keys().cloned().collect();
        let expected: std::collections::HashSet<String> =
            ["id", "rootPath", "worktreePath", "isBare", "headState", "currentBranch"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn detached_and_unborn_head_states_map_distinctly() {
        let detached = HeadStateDto::from(&HeadState::Detached {
            commit: CommitHash::new("a".repeat(40)).unwrap(),
        });
        let unborn = HeadStateDto::from(&HeadState::Unborn);

        assert_eq!(serde_json::to_value(&detached).unwrap()["state"], "detached");
        assert_eq!(serde_json::to_value(&unborn).unwrap()["state"], "unborn");
    }

    #[test]
    fn commit_dto_surfaces_derived_root_and_merge_flags() {
        let commit = Commit {
            hash: CommitHash::new("a".repeat(40)).unwrap(),
            short_hash: ShortHash::new("aaaaaaaa").unwrap(),
            parents: vec![],
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: "root commit".into(),
            body: String::new(),
            decorations: vec![],
        };

        let dto = CommitDto::from(&commit);
        assert!(dto.is_root);
        assert!(!dto.is_merge);
    }

    #[test]
    fn blame_dto_echoes_the_queried_file_and_revision() {
        let blame = Blame {
            file: PathBuf::from("src/lib.rs"),
            revision: None,
            lines: vec![],
        };
        let dto = BlameDto::from(&blame);
        assert_eq!(dto.file, "src/lib.rs");
        assert_eq!(dto.revision, None);
    }
}
