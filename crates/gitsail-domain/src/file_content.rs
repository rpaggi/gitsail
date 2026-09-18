//! A file's full content as of a specific committed revision (EPIC-15/US-076).

use std::path::PathBuf;

use crate::ids::CommitHash;

/// The outcome of reading one file's content at a revision: `Text` for
/// UTF-8-representable content, `Binary` when it is not, and `Missing` when
/// the path simply did not exist in that revision's tree. All three are
/// legitimate, expected outcomes of a query — never a
/// [`crate::error::GitSailError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentKind {
    Text(String),
    Binary,
    Missing,
}

/// One file's content as of a specific committed revision (EPIC-15/US-076),
/// e.g. to open a historical version read-only. Unlike [`crate::Blame`],
/// there is no working-tree mode: `revision` is always a resolved commit.
///
/// `path` and `revision` echo back what was actually queried, matching the
/// same "conteúdo consultado e revisão são identificáveis pela interface"
/// convention [`crate::Blame`] follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentAtRevision {
    pub path: PathBuf,
    pub revision: CommitHash,
    pub kind: FileContentKind,
}
