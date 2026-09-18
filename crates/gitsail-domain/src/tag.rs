//! Tag entity (SAD §8; EPIC-18/T-216/US-091, T-219/US-094).

use crate::commit::{GitTimestamp, Signature};
use crate::ids::CommitHash;

/// Tag-type-specific metadata (US-091 criterion 1: "tags incluem alvo
/// (commit) e metadados disponíveis (anotada: mensagem/tagger/data; leve: só
/// o alvo — trate os dois tipos)"). A lightweight tag is just a name
/// pointing at a commit, with no message/tagger/date of its own; an
/// annotated tag is a real Git object carrying all three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagKind {
    Lightweight,
    Annotated {
        message: String,
        tagger: Signature,
        date: GitTimestamp,
    },
}

/// A single local tag reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The commit this tag ultimately resolves to. For an annotated tag,
    /// this is the tag object peeled down to the commit it names (US-091
    /// criterion 1) — never the intermediate tag object id.
    pub target: CommitHash,
    pub kind: TagKind,
}
