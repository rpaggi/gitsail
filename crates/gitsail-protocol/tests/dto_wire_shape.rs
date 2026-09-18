//! Pins the exact JSON shape of a representative DTO for each
//! `#[serde(...)]` pattern used across `dto.rs` (US-039 criterion 1).
//!
//! Rust's own type system already catches most *added* fields (a `From`
//! impl building the DTO via a struct literal fails to compile once a
//! field is missing), but it does nothing to stop a silent wire-shape
//! change that needs no Rust-side edit anywhere else: renaming a
//! `#[serde(rename = "...")]`, dropping `rename_all`, changing a `tag`, or
//! swapping a field's `skip_serializing_if`. Those are exactly the
//! changes `SCHEMA_VERSION`'s policy (see `envelope.rs`) cares about, so
//! this test asserts the serialized `serde_json::Value` byte-for-byte:
//! touch any of those attributes and this test fails right here, forcing
//! a conscious answer to "does this need `SCHEMA_VERSION` bumped?" instead
//! of shipping a silently reshaped wire format.
//!
//! Not exhaustive over every DTO — that would mostly duplicate `dto.rs`
//! itself — but covers one example of: an externally-tagged enum with unit
//! and struct variants (`HeadStateDto`), a `rename_all = "camelCase"`
//! struct with `Option` fields serialized as `null` when absent
//! (`RepositoryDto`, `BranchDto`), a struct with a `skip_serializing_if`
//! optional field that is *omitted* (not `null`) when absent
//! (`CommitGraphPageDto`), a simple `rename_all = "snake_case"` enum
//! (`DiffLineOriginDto`), and a struct nesting other DTOs and a `Vec` of a
//! tagged enum (`CommitDto`).

use gitsail_protocol::{
    BranchDto, BranchKindDto, CommitDto, CommitGraphPageDto, DecorationDto, DiffLineOriginDto,
    GitTimestampDto, HeadStateDto, PullOutcomeDto, RepositoryDto, SignatureDto, SyncTargetDto,
};
use serde_json::json;

#[test]
fn head_state_dto_is_externally_tagged_with_camel_case_variant_fields() {
    let attached = HeadStateDto::Attached {
        branch: "main".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&attached).unwrap(),
        json!({ "state": "attached", "branch": "main" })
    );

    let detached = HeadStateDto::Detached {
        commit: "deadbeef".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&detached).unwrap(),
        json!({ "state": "detached", "commit": "deadbeef" })
    );

    let unborn = HeadStateDto::Unborn;
    assert_eq!(
        serde_json::to_value(&unborn).unwrap(),
        json!({ "state": "unborn" })
    );
}

#[test]
fn repository_dto_uses_camel_case_field_names_and_keeps_absent_optionals_null() {
    let repo = RepositoryDto {
        id: "repo-1".to_string(),
        root_path: "/work/gitsail".to_string(),
        worktree_path: None,
        is_bare: true,
        head_state: HeadStateDto::Unborn,
        current_branch: None,
    };

    assert_eq!(
        serde_json::to_value(&repo).unwrap(),
        json!({
            "id": "repo-1",
            "rootPath": "/work/gitsail",
            "worktreePath": null,
            "isBare": true,
            "headState": { "state": "unborn" },
            "currentBranch": null,
        })
    );
}

#[test]
fn branch_dto_serializes_an_absent_upstream_as_null_not_omitted() {
    let branch = BranchDto {
        name: "main".to_string(),
        kind: BranchKindDto::Local,
        target: "deadbeef".to_string(),
        upstream: None,
        ahead: 0,
        behind: 0,
        is_current: true,
    };

    assert_eq!(
        serde_json::to_value(&branch).unwrap(),
        json!({
            "name": "main",
            "kind": { "kind": "local" },
            "target": "deadbeef",
            "upstream": null,
            "ahead": 0,
            "behind": 0,
            "isCurrent": true,
        })
    );
}

#[test]
fn commit_graph_page_dto_omits_next_cursor_when_absent_instead_of_serializing_null() {
    let page = CommitGraphPageDto {
        rows: vec![],
        lane_count: 1,
        has_more: false,
        next_cursor: None,
    };
    let value = serde_json::to_value(&page).unwrap();

    assert_eq!(
        value,
        json!({ "rows": [], "laneCount": 1, "hasMore": false })
    );
    assert!(
        value.get("nextCursor").is_none(),
        "next_cursor has skip_serializing_if today: it must be omitted, not null, \
         when absent — changing that is a wire-shape change consumers may rely on \
         (e.g. `\"nextCursor\" in response` presence checks), so pin it explicitly"
    );
}

#[test]
fn diff_line_origin_dto_is_a_plain_snake_case_string_enum() {
    assert_eq!(
        serde_json::to_value(DiffLineOriginDto::Context).unwrap(),
        json!("context")
    );
    assert_eq!(
        serde_json::to_value(DiffLineOriginDto::Addition).unwrap(),
        json!("addition")
    );
    assert_eq!(
        serde_json::to_value(DiffLineOriginDto::Deletion).unwrap(),
        json!("deletion")
    );
}

#[test]
fn pull_outcome_dto_is_internally_tagged_with_an_explicitly_renamed_struct_variant_field() {
    // The one DTO in this crate needing an explicit per-field `rename`
    // inside an enum variant (`rename_all` on an enum only renames variant
    // names, not struct-variant fields) — pinned here since that is easy to
    // silently drop in a future edit.
    assert_eq!(
        serde_json::to_value(PullOutcomeDto::AlreadyUpToDate).unwrap(),
        json!({ "outcome": "alreadyUpToDate" })
    );
    assert_eq!(
        serde_json::to_value(PullOutcomeDto::FastForwarded {
            new_head: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string()
        })
        .unwrap(),
        json!({
            "outcome": "fastForwarded",
            "newHead": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        })
    );
}

#[test]
fn sync_target_dto_serializes_an_absent_branch_as_null() {
    let target = SyncTargetDto {
        remote: "origin".to_string(),
        branch: None,
    };

    assert_eq!(
        serde_json::to_value(&target).unwrap(),
        json!({ "remote": "origin", "branch": null })
    );
}

#[test]
fn commit_dto_nests_signatures_timestamps_and_tagged_decorations() {
    let commit = CommitDto {
        hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        short_hash: "deadbeef".to_string(),
        parents: vec!["cafefeed".to_string()],
        author: SignatureDto {
            name: "Ada".to_string(),
            email: "ada@example.com".to_string(),
        },
        committer: SignatureDto {
            name: "Ada".to_string(),
            email: "ada@example.com".to_string(),
        },
        author_date: GitTimestampDto {
            seconds_since_epoch: 1_700_000_000,
            utc_offset_minutes: -180,
        },
        commit_date: GitTimestampDto {
            seconds_since_epoch: 1_700_000_100,
            utc_offset_minutes: -180,
        },
        subject: "Add feature".to_string(),
        body: String::new(),
        decorations: vec![
            DecorationDto::Head,
            DecorationDto::Tag {
                name: "v1.0".to_string(),
            },
        ],
        is_merge: false,
        is_root: false,
    };

    assert_eq!(
        serde_json::to_value(&commit).unwrap(),
        json!({
            "hash": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "shortHash": "deadbeef",
            "parents": ["cafefeed"],
            "author": { "name": "Ada", "email": "ada@example.com" },
            "committer": { "name": "Ada", "email": "ada@example.com" },
            "authorDate": { "secondsSinceEpoch": 1_700_000_000i64, "utcOffsetMinutes": -180 },
            "commitDate": { "secondsSinceEpoch": 1_700_000_100i64, "utcOffsetMinutes": -180 },
            "subject": "Add feature",
            "body": "",
            "decorations": [
                { "kind": "head" },
                { "kind": "tag", "name": "v1.0" }
            ],
            "isMerge": false,
            "isRoot": false,
        })
    );
}
