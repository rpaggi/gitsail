//! Parses the free-text query typed into the Graph panel's commit-search
//! box (US-045 criterion 2) into [`CommitQuery`] filters — deliberately
//! never a TUI-only text match: every prefix below maps onto a field
//! [`CommitQuery`] (US-017) already exposes, so the exact same query would
//! also work unchanged from the CLI or Desktop once either grows its own
//! search UI (per `AGENTS.md`'s "don't invent filtering logic that already
//! exists in Core").
//!
//! Kept as a plain, Ratatui-free function — like [`crate::graph_view`] and
//! [`crate::status_view`] — so the parsing rules are testable with plain
//! string assertions, independent of [`crate::app::App`] or a terminal.
//!
//! # Supported syntax
//!
//! Checked in this order, case-insensitively, first match wins:
//!
//! - `author:<text>` -> [`CommitQuery::author`] (substring match on
//!   name/email, Git's `--author`).
//! - `branch:<name>` -> [`CommitQuery::branch`]. An empty name (nothing
//!   after the prefix) is invalid and falls back to the unfiltered query
//!   rather than failing the search outright — a search box is not the
//!   place to surface a branch-name validation error.
//! - `hash:<rev>`, or bare input that already looks like a hex commit hash
//!   (4-40 hex digits, same as an abbreviated Git hash) ->
//!   [`CommitQuery::revision_range`]: Git accepts an abbreviated hash
//!   directly as a revision, so this shows that commit and its ancestors.
//! - anything else -> [`CommitQuery::text_query`] (message substring,
//!   Git's `--grep`).
//!
//! Blank (or whitespace-only) input clears every filter, producing the same
//! unfiltered query [`crate::app::App::on_repository_opened`] requests for
//! the very first page.

use gitsail_application::CommitQuery;
use gitsail_domain::BranchName;

pub fn parse_commit_search(text: &str) -> CommitQuery {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CommitQuery::default();
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "author:") {
        return CommitQuery {
            author: Some(rest.trim().to_string()),
            ..CommitQuery::default()
        };
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "branch:") {
        return CommitQuery {
            branch: BranchName::new(rest.trim()).ok(),
            ..CommitQuery::default()
        };
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "hash:") {
        return CommitQuery {
            revision_range: Some(rest.trim().to_string()),
            ..CommitQuery::default()
        };
    }
    if looks_like_hash(trimmed) {
        return CommitQuery {
            revision_range: Some(trimmed.to_string()),
            ..CommitQuery::default()
        };
    }
    CommitQuery {
        text_query: Some(trimmed.to_string()),
        ..CommitQuery::default()
    }
}

/// Case-insensitive `str::strip_prefix`, safe against `text` containing a
/// multi-byte character right at `prefix`'s byte length (`str::get` never
/// panics on a non-boundary index, unlike slicing).
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        text.get(prefix.len()..)
    } else {
        None
    }
}

/// Whether `text` is plausibly a (possibly abbreviated) Git commit hash:
/// Git's shortest accepted abbreviation is 4 hex digits, and a full SHA-1
/// hash is 40.
fn looks_like_hash(text: &str) -> bool {
    (4..=40).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_input_clears_every_filter() {
        assert_eq!(parse_commit_search(""), CommitQuery::default());
        assert_eq!(parse_commit_search("   "), CommitQuery::default());
    }

    #[test]
    fn plain_text_searches_the_commit_message() {
        let query = parse_commit_search("fix login bug");
        assert_eq!(query.text_query, Some("fix login bug".to_string()));
        assert_eq!(query.author, None);
        assert_eq!(query.branch, None);
        assert_eq!(query.revision_range, None);
    }

    #[test]
    fn author_prefix_is_case_insensitive_and_trims_the_value() {
        let query = parse_commit_search("AUTHOR:  Ada Lovelace  ");
        assert_eq!(query.author, Some("Ada Lovelace".to_string()));
        assert_eq!(query.text_query, None);
    }

    #[test]
    fn branch_prefix_sets_the_branch_filter() {
        let query = parse_commit_search("branch:feature/x");
        assert_eq!(
            query.branch.as_ref().map(BranchName::as_str),
            Some("feature/x")
        );
    }

    #[test]
    fn an_empty_branch_name_falls_back_to_unfiltered() {
        let query = parse_commit_search("branch:");
        assert_eq!(query.branch, None);
        assert_eq!(query, CommitQuery::default());
    }

    #[test]
    fn explicit_hash_prefix_sets_the_revision_range() {
        let query = parse_commit_search("hash:deadbeef");
        assert_eq!(query.revision_range, Some("deadbeef".to_string()));
    }

    #[test]
    fn a_bare_hex_string_is_treated_as_a_hash_without_a_prefix() {
        let query = parse_commit_search("deadbeef");
        assert_eq!(query.revision_range, Some("deadbeef".to_string()));
        assert_eq!(query.text_query, None);
    }

    #[test]
    fn a_short_hex_string_below_the_minimum_abbreviation_is_a_message_search() {
        // Git's shortest accepted abbreviation is 4 hex digits.
        let query = parse_commit_search("abc");
        assert_eq!(query.text_query, Some("abc".to_string()));
        assert_eq!(query.revision_range, None);
    }

    #[test]
    fn a_full_length_hash_is_recognized() {
        let hash = "a".repeat(40);
        let query = parse_commit_search(&hash);
        assert_eq!(query.revision_range, Some(hash));
    }

    #[test]
    fn non_hex_text_never_becomes_a_revision_range_even_if_short() {
        let query = parse_commit_search("fix");
        assert_eq!(query.text_query, Some("fix".to_string()));
        assert_eq!(query.revision_range, None);
    }
}
