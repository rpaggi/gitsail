//! Rendering a command's result as human-readable text or as protocol DTOs
//! ready to embed in a JSON [`gitsail_protocol::Envelope`] (US-036, US-037).

use std::fmt::Write as _;

use gitsail_protocol::{
    BlameDto, BlameOriginDto, BranchDto, BranchKindDto, CommitDto, DiffDto, DiffLineOriginDto,
    HeadStateDto, Page, RepositoryDto, RepositoryStatusDto,
};
use serde::Serialize;

/// The result of any of the six query commands (US-036).
///
/// `#[serde(untagged)]` means this enum contributes no wrapper of its own to
/// the JSON it produces: serializing an `Output` yields exactly the inner
/// DTO's shape, so it can be dropped straight into an `Envelope<Output>`'s
/// `data` field without an extra nesting level a consumer would have to
/// unwrap (US-035 criterion 1).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Output {
    Repository(RepositoryDto),
    Status(RepositoryStatusDto),
    Commits(Page<CommitDto>),
    Branches(Vec<BranchDto>),
    Diff(DiffDto),
    Blame(BlameDto),
}

impl Output {
    /// Renders this result as the text `gitsail` prints without `--json`
    /// (US-036 criterion 1: consistent context, legible output).
    pub fn render_human(&self) -> String {
        match self {
            Self::Repository(repo) => render_repository(repo),
            Self::Status(status) => render_status(status),
            Self::Commits(page) => render_commits(page),
            Self::Branches(branches) => render_branches(branches),
            Self::Diff(diff) => render_diff(diff),
            Self::Blame(blame) => render_blame(blame),
        }
    }
}

fn render_head_state(state: &HeadStateDto) -> String {
    match state {
        HeadStateDto::Attached { branch } => format!("attached to {branch}"),
        HeadStateDto::Detached { commit } => format!("detached at {}", &commit[..commit.len().min(12)]),
        HeadStateDto::Unborn => "unborn (no commits yet)".to_string(),
    }
}

fn render_repository(repo: &RepositoryDto) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "root:   {}", repo.root_path);
    if let Some(worktree) = &repo.worktree_path {
        let _ = writeln!(out, "worktree: {worktree}");
    } else {
        let _ = writeln!(out, "worktree: (none, bare repository)");
    }
    let _ = writeln!(out, "bare:   {}", repo.is_bare);
    let _ = writeln!(out, "HEAD:   {}", render_head_state(&repo.head_state));
    if let Some(branch) = &repo.current_branch {
        let _ = writeln!(out, "branch: {branch}");
    }
    out
}

fn render_status(status: &RepositoryStatusDto) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "HEAD: {}", render_head_state(&status.head_state));
    if status.is_clean {
        let _ = writeln!(out, "nothing to report, working tree clean");
        return out;
    }
    for file in &status.files {
        let index = status_letter(file.index_status);
        let worktree = status_letter(file.worktree_status);
        match &file.previous_path {
            Some(previous) => {
                let _ = writeln!(out, "{index}{worktree} {previous} -> {}", file.path);
            }
            None => {
                let _ = writeln!(out, "{index}{worktree} {}", file.path);
            }
        }
    }
    out
}

fn status_letter(code: gitsail_protocol::FileStatusCodeDto) -> char {
    use gitsail_protocol::FileStatusCodeDto::*;
    match code {
        Unmodified => '.',
        Modified => 'M',
        Added => 'A',
        Deleted => 'D',
        Renamed => 'R',
        Copied => 'C',
        UpdatedButUnmerged => 'U',
        Untracked => '?',
        Ignored => '!',
    }
}

fn render_commits(page: &Page<CommitDto>) -> String {
    let mut out = String::new();
    for commit in &page.items {
        let _ = writeln!(out, "commit {}", commit.hash);
        let _ = writeln!(out, "Author: {} <{}>", commit.author.name, commit.author.email);
        let _ = writeln!(out, "Date:   {}", commit.author_date.seconds_since_epoch);
        let _ = writeln!(out);
        for line in commit.subject.lines() {
            let _ = writeln!(out, "    {line}");
        }
        let _ = writeln!(out);
    }
    if page.has_more {
        let _ = writeln!(
            out,
            "-- more commits available, continue with --cursor {} --",
            page.next_cursor.as_deref().unwrap_or_default()
        );
    }
    out
}

fn render_branches(branches: &[BranchDto]) -> String {
    let mut out = String::new();
    for branch in branches {
        let marker = if branch.is_current { '*' } else { ' ' };
        let kind = match &branch.kind {
            BranchKindDto::Local => branch.name.clone(),
            BranchKindDto::Remote { remote } => format!("{remote}/{}", branch.name),
        };
        let tracking = match (&branch.upstream, branch.ahead, branch.behind) {
            (Some(upstream), ahead, behind) => {
                format!(" [{upstream}: ahead {ahead}, behind {behind}]")
            }
            (None, _, _) => String::new(),
        };
        let _ = writeln!(out, "{marker} {kind}{tracking}");
    }
    out
}

fn render_diff(diff: &DiffDto) -> String {
    let mut out = String::new();
    if diff.files.is_empty() {
        let _ = writeln!(out, "no differences");
        return out;
    }
    for file in &diff.files {
        let header = match &file.previous_path {
            Some(previous) => format!("{previous} -> {}", file.path),
            None => file.path.clone(),
        };
        let _ = writeln!(out, "diff -- {header} ({:?})", file.change_type);
        if file.is_binary {
            let _ = writeln!(out, "  (binary file)");
            continue;
        }
        if file.truncated {
            let _ = writeln!(out, "  (diff truncated: too large to display)");
            continue;
        }
        for hunk in &file.hunks {
            let _ = writeln!(
                out,
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            );
            for line in &hunk.lines {
                let sigil = match line.origin {
                    DiffLineOriginDto::Context => ' ',
                    DiffLineOriginDto::Addition => '+',
                    DiffLineOriginDto::Deletion => '-',
                };
                let _ = writeln!(out, "{sigil}{}", line.content);
            }
        }
    }
    out
}

fn render_blame(blame: &BlameDto) -> String {
    let mut out = String::new();
    for line in &blame.lines {
        let marker = match line.origin {
            BlameOriginDto::Committed => &line.commit[..line.commit.len().min(8)],
            BlameOriginDto::Local => "local",
        };
        let _ = writeln!(
            out,
            "{marker} ({:<20} {:>5}) {}",
            line.author.name, line.final_line, line.content
        );
    }
    out
}
