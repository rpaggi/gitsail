//! Dispatches a parsed [`Command`] to the matching application use case and
//! maps its result into an [`Output`] DTO (US-036).

use std::sync::Arc;

use gitsail_application::{
    BlameRequest, CommitQuery, CompareRevisions, DiffRequest, GetCommitHistory, GetDiff,
    GetFileBlame, GetRepositoryStatus, ListBranches, OpenRepository, RepositoryReadPort,
};
use gitsail_domain::{CancellationToken, ErrorCode, GitSailError, LineRange};
use gitsail_protocol::{BlameDto, BranchDto, CommitDto, DiffDto, Page, RepositoryDto, RepositoryStatusDto};

use crate::cli::{BlameArgs, Command, DiffArgs};
use crate::output::Output;

pub fn execute(
    port: &Arc<dyn RepositoryReadPort>,
    repo_path: &std::path::Path,
    command: &Command,
    cancel: &CancellationToken,
) -> Result<Output, GitSailError> {
    let repo = OpenRepository::new(port.clone()).execute(repo_path)?;

    match command {
        Command::Open => Ok(Output::Repository(RepositoryDto::from(&repo))),

        Command::Status => {
            let status = GetRepositoryStatus::new(port.clone()).execute(&repo)?;
            Ok(Output::Status(RepositoryStatusDto::from(&status)))
        }

        Command::Log(args) => {
            let query = CommitQuery {
                limit: args.limit,
                cursor: args.cursor.clone(),
                revision_range: args.revision.clone(),
                branch: None,
                author: args.author.clone(),
                text_query: args.text_query.clone(),
                path_filter: args.path_filter.clone(),
                follow_renames: !args.no_follow,
            };
            let page = GetCommitHistory::new(port.clone()).execute(&repo, &query)?;
            let items = page.items.iter().map(CommitDto::from).collect();
            Ok(Output::Commits(Page::new(items, page.next_cursor, page.has_more)))
        }

        Command::Branches => {
            let branches = ListBranches::new(port.clone()).execute(&repo)?;
            Ok(Output::Branches(branches.iter().map(BranchDto::from).collect()))
        }

        Command::Diff(args) => {
            let diff = execute_diff(port, &repo, args, cancel)?;
            Ok(Output::Diff(DiffDto::from(&diff)))
        }

        Command::Blame(args) => {
            let blame = execute_blame(port, &repo, args, cancel)?;
            Ok(Output::Blame(BlameDto::from(&blame)))
        }
    }
}

fn execute_diff(
    port: &Arc<dyn RepositoryReadPort>,
    repo: &gitsail_domain::Repository,
    args: &DiffArgs,
    cancel: &CancellationToken,
) -> Result<gitsail_domain::Diff, GitSailError> {
    match (&args.base, &args.target) {
        (None, None) => {
            let request = DiffRequest {
                staged: args.staged,
                path_filter: args.path_filter.clone(),
                context_lines: args.context,
                ..DiffRequest::default()
            };
            GetDiff::new(port.clone()).execute(repo, &request, cancel)
        }
        (Some(_), Some(_)) if args.staged => Err(usage_error(
            "cannot combine two revisions with --staged",
            "pass either two revisions or --staged, not both",
        )),
        (Some(base), Some(target)) => {
            let comparison =
                CompareRevisions::new(port.clone()).execute(repo, base, target, cancel)?;
            let request = DiffRequest {
                from: Some(comparison.base),
                to: Some(comparison.target),
                staged: false,
                path_filter: args.path_filter.clone(),
                context_lines: args.context,
            };
            GetDiff::new(port.clone()).execute(repo, &request, cancel)
        }
        (Some(_), None) if args.staged => Err(usage_error(
            "cannot combine a revision with --staged",
            "pass either a revision or --staged, not both",
        )),
        (Some(base), None) => {
            let from = port.resolve_revision(repo, base)?;
            let request = DiffRequest {
                from: Some(from),
                to: None,
                staged: false,
                path_filter: args.path_filter.clone(),
                context_lines: args.context,
            };
            GetDiff::new(port.clone()).execute(repo, &request, cancel)
        }
        (None, Some(_)) => Err(usage_error(
            "a target revision requires a base revision",
            "pass `gitsail diff <base> <target>`, or `gitsail diff <base>` alone",
        )),
    }
}

fn execute_blame(
    port: &Arc<dyn RepositoryReadPort>,
    repo: &gitsail_domain::Repository,
    args: &BlameArgs,
    cancel: &CancellationToken,
) -> Result<gitsail_domain::Blame, GitSailError> {
    let revision = args
        .revision
        .as_deref()
        .map(|rev| port.resolve_revision(repo, rev))
        .transpose()?;
    let line_range = args.range.as_deref().map(parse_line_range).transpose()?;

    let request = BlameRequest {
        file: args.file.clone(),
        revision,
        line_range,
        buffer_contents: None,
    };
    // A fresh cache per invocation is fine: a CLI process runs exactly one
    // query, so `GetFileBlame`'s cache never has a second call to serve.
    GetFileBlame::new(port.clone()).execute(repo, &request, 0, cancel)
}

fn parse_line_range(text: &str) -> Result<LineRange, GitSailError> {
    let (start, end) = text.split_once('-').ok_or_else(|| {
        usage_error(
            format!("invalid --range '{text}': expected START-END"),
            "use an inclusive 1-based range, e.g. --range 10-25",
        )
    })?;
    let parse_bound = |s: &str| -> Result<u32, GitSailError> {
        s.trim().parse::<u32>().map_err(|_| {
            usage_error(
                format!("invalid --range '{text}': '{s}' is not a positive integer"),
                "use an inclusive 1-based range, e.g. --range 10-25",
            )
        })
    };
    let range = LineRange::new(parse_bound(start)?, parse_bound(end)?);
    if !range.is_valid() {
        return Err(usage_error(
            format!("invalid --range '{text}': start must be at least 1 and not greater than end"),
            "use an inclusive 1-based range, e.g. --range 10-25",
        ));
    }
    Ok(range)
}

fn usage_error(message: impl Into<String>, remediation: impl Into<String>) -> GitSailError {
    GitSailError::new(ErrorCode::ParseFailure, message).with_remediation(remediation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_inclusive_range() {
        let range = parse_line_range("10-25").unwrap();
        assert_eq!(range.start, 10);
        assert_eq!(range.end, 25);
    }

    #[test]
    fn rejects_malformed_or_inverted_ranges() {
        assert!(parse_line_range("not-a-range").is_err());
        assert!(parse_line_range("25-10").is_err());
        assert!(parse_line_range("0-10").is_err());
        assert!(parse_line_range("abc-def").is_err());
    }
}
