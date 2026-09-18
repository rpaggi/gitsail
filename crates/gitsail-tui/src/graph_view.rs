//! Pure text rendering for the Graph panel (US-066), decoupled from
//! Ratatui so it is testable with plain string assertions rather than a
//! terminal buffer — the same split [`crate::status_view`] already uses.
//!
//! [`render_row`] never recomputes lanes or edges: it only reads what
//! [`gitsail_domain::CommitGraph`] already calculated (US-066 criterion 1),
//! turning each [`gitsail_domain::GraphRow`] into one line of text. Every
//! lane-changing edge is drawn as a distinct connector glyph (never color
//! alone — US-066 criterion 2), and an edge this page could not resolve is
//! called out in plain text (`(continues…)`) rather than silently vanishing
//! (US-065 criterion 1).
//!
//! This intentionally draws each lane transition on the commit's own row
//! rather than on a separate connector row between commits: a merge/branch
//! point is legible at the row where it happens, and every row/lane's
//! vertical continuity is still fully recoverable from
//! `passthrough_lanes` alone. A future story wanting the denser two-row
//! `git log --graph` diagonal style can build it directly on
//! [`gitsail_domain::GraphRow`] without changing that type.

use gitsail_domain::{Commit, Decoration, GraphRow};

use crate::sanitize;

/// One rendered line of the graph, tied back to the commit it represents so
/// [`crate::app::App`] can map a cursor position to a hash unambiguously
/// (US-066 criterion 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLine {
    pub text: String,
    pub commit: gitsail_domain::CommitHash,
}

fn node_glyph(commit: &Commit) -> char {
    if commit.is_merge() {
        '◆'
    } else if commit.is_root() {
        '○'
    } else {
        '●'
    }
}

fn decoration_label(decoration: &Decoration) -> String {
    match decoration {
        Decoration::Head => "HEAD".to_string(),
        Decoration::Branch(name) => name.as_str().to_string(),
        Decoration::RemoteBranch { remote, branch } => format!("{remote}/{}", branch.as_str()),
        Decoration::Tag(name) => format!("tag: {name}"),
    }
}

/// Renders the lane-track prefix for `row`: one glyph per lane column,
/// `lane_count` wide, each followed by a single space separator.
fn lane_track(row: &GraphRow, commit: &Commit, lane_count: usize) -> String {
    let mut track = String::with_capacity(lane_count * 2);
    for lane in 0..lane_count {
        let glyph = if lane == row.lane {
            node_glyph(commit)
        } else if row.passthrough_lanes.contains(&lane) {
            '│'
        } else if row.edges.iter().any(|edge| edge.to_lane == lane) {
            // A lane this row's commit connects to but does not itself
            // occupy: a branch spawning to the right, or a merge
            // converging back to the left.
            if lane > row.lane {
                '\\'
            } else {
                '/'
            }
        } else {
            ' '
        };
        track.push(glyph);
        track.push(' ');
    }
    track
}

/// Renders one [`GraphLine`] for `row`/`commit` — a matched pair from the
/// same index of [`gitsail_domain::CommitGraph::rows`] and the parallel
/// commit list [`crate::app::App`] keeps alongside it.
pub fn render_row(row: &GraphRow, commit: &Commit, lane_count: usize) -> GraphLine {
    let mut text = lane_track(row, commit, lane_count);
    text.push_str(&sanitize::safe_line(commit.short_hash.as_str()));

    if !row.decorations.is_empty() {
        let labels: Vec<String> = row.decorations.iter().map(decoration_label).collect();
        text.push_str(&format!(" ({})", sanitize::safe_line(&labels.join(", "))));
    }

    text.push(' ');
    text.push_str(&sanitize::safe_line(&commit.subject));

    if row.edges.iter().any(|edge| !edge.resolved) {
        text.push_str(" (continues…)");
    }

    GraphLine {
        text,
        commit: commit.hash.clone(),
    }
}

/// Renders every row in `rows`, paired index-for-index with `commits` (both
/// must have the same length — [`gitsail_domain::CommitGraph::append_page`]
/// always returns exactly one row per input commit, in order, so a caller
/// that keeps a parallel `Vec<Commit>` alongside its `CommitGraph` can
/// always zip them like this).
pub fn render_rows(rows: &[GraphRow], commits: &[Commit], lane_count: usize) -> Vec<GraphLine> {
    rows.iter()
        .zip(commits.iter())
        .map(|(row, commit)| render_row(row, commit, lane_count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{BranchName, CommitHash, GitTimestamp, GraphCommit, Signature};

    /// Places `seed` in the leading hex digits so the derived short hash
    /// (the first 8 hex characters) is distinct and readable per seed,
    /// rather than every low seed value collapsing to `"00000000"` under
    /// left-padded hex formatting.
    fn hash(seed: u8) -> CommitHash {
        CommitHash::new(format!("{seed:02x}{:038x}", 0)).unwrap()
    }

    fn short(seed: u8) -> String {
        hash(seed).to_short(8).as_str().to_string()
    }

    fn commit(seed: u8, parents: &[u8], subject: &str, decorations: Vec<Decoration>) -> Commit {
        Commit {
            hash: hash(seed),
            short_hash: hash(seed).to_short(8),
            parents: parents.iter().map(|p| hash(*p)).collect(),
            author: Signature::new("Ada", "ada@example.com"),
            committer: Signature::new("Ada", "ada@example.com"),
            author_date: GitTimestamp::new(0, 0),
            commit_date: GitTimestamp::new(0, 0),
            subject: subject.to_string(),
            body: String::new(),
            decorations,
        }
    }

    #[test]
    fn a_root_commit_renders_its_own_glyph_and_no_continuation_marker() {
        let commit = commit(1, &[], "initial commit", vec![]);
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&commit)]);

        let line = render_row(&graph.rows()[0], &commit, graph.lane_count());

        assert_eq!(line.text, format!("○ {} initial commit", short(1)));
        assert_eq!(line.commit, commit.hash);
    }

    #[test]
    fn a_normal_commit_shows_a_plain_node_glyph() {
        let commit = commit(2, &[1], "second commit", vec![]);
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&commit)]);

        let line = render_row(&graph.rows()[0], &commit, graph.lane_count());

        // The parent (hash(1)) has not loaded yet, so the edge is a
        // continuation, called out in plain text rather than a color.
        assert_eq!(
            line.text,
            format!("● {} second commit (continues…)", short(2))
        );
    }

    #[test]
    fn decorations_are_shown_next_to_the_hash() {
        let commit = commit(
            3,
            &[],
            "release",
            vec![
                Decoration::Branch(BranchName::new("main").unwrap()),
                Decoration::Tag("v1.0".into()),
            ],
        );
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&commit)]);

        let line = render_row(&graph.rows()[0], &commit, graph.lane_count());

        assert_eq!(
            line.text,
            format!("○ {} (main, tag: v1.0) release", short(3))
        );
    }

    #[test]
    fn a_merge_draws_a_diagonal_to_the_lane_it_spawns() {
        let merge = commit(0xAA, &[0xA1, 0xB1], "merge branch", vec![]);
        let a = commit(0xA1, &[], "a", vec![]);
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&merge), GraphCommit::from(&a)]);

        let line = render_row(&graph.rows()[0], &merge, graph.lane_count());

        assert_eq!(graph.lane_count(), 2);
        // Lane 0 is the merge's own node; lane 1 is the second parent's
        // freshly spawned lane, drawn as a diagonal since 1 > 0.
        let backslash = '\\';
        assert_eq!(
            line.text,
            format!("◆ {backslash} {} merge branch (continues…)", short(0xAA))
        );
    }

    #[test]
    fn passthrough_lanes_render_as_a_plain_vertical_bar() {
        // Row 0 (b, a merge) resolves lane 0 with its first parent (a) and
        // spawns lane 1 for its second parent (still unloaded); row 1 (a)
        // then sits on lane 0 while lane 1 passes straight through,
        // untouched by `a` (a root commit, so it has no edges of its own).
        let b = commit(0xB2, &[0xA1, 0xC1], "merge", vec![]);
        let a = commit(0xA1, &[], "a", vec![]);
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&b), GraphCommit::from(&a)]);

        let row_a = &graph.rows()[1];
        assert_eq!(row_a.passthrough_lanes, vec![1]);
        let line = render_row(row_a, &a, graph.lane_count());
        assert_eq!(line.text, format!("○ │ {} a", short(0xA1)));
    }

    #[test]
    fn render_rows_pairs_rows_and_commits_by_index() {
        let c2 = commit(2, &[1], "second", vec![]);
        let c1 = commit(1, &[], "first", vec![]);
        let mut graph = gitsail_domain::CommitGraph::new();
        graph.append_page(&[GraphCommit::from(&c2), GraphCommit::from(&c1)]);

        let lines = render_rows(graph.rows(), &[c2.clone(), c1.clone()], graph.lane_count());

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].commit, c2.hash);
        assert_eq!(lines[1].commit, c1.hash);
        assert!(
            !lines[1].text.contains("continues"),
            "the parent resolved within the same page"
        );
    }
}
