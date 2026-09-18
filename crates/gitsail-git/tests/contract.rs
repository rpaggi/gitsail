//! T-253/US-120 criterion 2: runs `gitsail-test-support`'s generic
//! `RepositoryReadPort` contract suite against `GitCliProvider` — today's
//! only implementation of that trait. Every `#[test]` here is a thin
//! one-line wrapper around a `gitsail_test_support::contract::*` function,
//! supplying `gitsail_test_support::provider` as the `make_provider`
//! factory; a future second `RepositoryReadPort` implementation (e.g. a
//! `libgit2`-backed adapter) reuses this exact same file's shape, just
//! swapping the factory.
//!
//! See `gitsail_test_support::contract`'s own module doc for what is (and,
//! deliberately, is not yet) covered, and for the locale-independence
//! argument behind US-120 criterion 3.

use gitsail_test_support::{contract, provider};

#[test]
fn discover_reports_root_and_unborn_head_for_a_fresh_repository() {
    contract::discover_reports_root_and_unborn_head_for_a_fresh_repository(provider);
}

#[test]
fn discover_fails_for_a_path_outside_any_repository() {
    contract::discover_fails_for_a_path_outside_any_repository(provider);
}

#[test]
fn discover_detects_a_detached_head() {
    contract::discover_detects_a_detached_head(provider);
}

#[test]
fn status_is_clean_right_after_a_commit() {
    contract::status_is_clean_right_after_a_commit(provider);
}

#[test]
fn status_reports_an_unstaged_modification_and_an_untracked_file() {
    contract::status_reports_an_unstaged_modification_and_an_untracked_file(provider);
}

#[test]
fn commits_lists_history_newest_first() {
    contract::commits_lists_history_newest_first(provider);
}

#[test]
fn commits_respects_a_limit_and_reports_more_remain() {
    contract::commits_respects_a_limit_and_reports_more_remain(provider);
}

#[test]
fn commit_reads_a_single_commit_matching_its_history_entry() {
    contract::commit_reads_a_single_commit_matching_its_history_entry(provider);
}

#[test]
fn commit_fails_for_an_unknown_hash() {
    contract::commit_fails_for_an_unknown_hash(provider);
}

#[test]
fn branches_lists_local_branches_with_exactly_one_current() {
    contract::branches_lists_local_branches_with_exactly_one_current(provider);
}

#[test]
fn diff_working_tree_reports_an_unstaged_modification() {
    contract::diff_working_tree_reports_an_unstaged_modification(provider);
}

#[test]
fn diff_staged_reports_only_the_index_not_the_working_tree() {
    contract::diff_staged_reports_only_the_index_not_the_working_tree(provider);
}

#[test]
fn diff_between_two_explicit_revisions() {
    contract::diff_between_two_explicit_revisions(provider);
}

#[test]
fn diff_reports_a_binary_file_without_fabricating_hunks() {
    contract::diff_reports_a_binary_file_without_fabricating_hunks(provider);
}

#[test]
fn resolve_revision_resolves_a_branch_name_to_its_tip() {
    contract::resolve_revision_resolves_a_branch_name_to_its_tip(provider);
}

#[test]
fn resolve_revision_fails_for_an_unresolvable_revision() {
    contract::resolve_revision_fails_for_an_unresolvable_revision(provider);
}

#[test]
fn blame_attributes_every_line_to_the_commit_that_introduced_it() {
    contract::blame_attributes_every_line_to_the_commit_that_introduced_it(provider);
}

#[test]
fn blame_working_tree_reports_local_origin_for_an_uncommitted_change() {
    contract::blame_working_tree_reports_local_origin_for_an_uncommitted_change(provider);
}

#[test]
fn line_history_traces_every_commit_that_touched_the_range() {
    contract::line_history_traces_every_commit_that_touched_the_range(provider);
}

#[test]
fn file_content_reads_historical_text_content() {
    contract::file_content_reads_historical_text_content(provider);
}

#[test]
fn file_content_reports_missing_for_a_path_added_later() {
    contract::file_content_reports_missing_for_a_path_added_later(provider);
}

#[test]
fn golden_parsing_preserves_a_file_name_with_special_characters() {
    contract::golden_parsing_preserves_a_file_name_with_special_characters(provider);
}

#[test]
fn golden_parsing_preserves_a_multi_line_commit_message() {
    contract::golden_parsing_preserves_a_multi_line_commit_message(provider);
}

#[test]
fn golden_parsing_preserves_non_ascii_commit_message_content() {
    contract::golden_parsing_preserves_non_ascii_commit_message_content(provider);
}
