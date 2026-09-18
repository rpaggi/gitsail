//! Commit graph layout: lanes and edges connecting commits by parentage
//! (US-064, US-065; SAD §8).
//!
//! [`CommitGraph`] computes the same structure `git log --graph` draws —
//! which vertical "lane" each commit occupies and how edges connect a
//! commit to its parents — but as plain data, independent of any
//! presentation toolkit (US-064 criterion 2): no Ratatui, Vue, or Tauri
//! dependency lives here or anywhere in this crate (SAD §4). `gitsail-tui`
//! and the Desktop bridge each turn this data into their own visual
//! representation; neither recomputes lanes or edges itself, so both always
//! agree with what the Core actually calculated.
//!
//! # Lane stability policy (US-065 criterion 3)
//!
//! [`CommitGraph`] is built incrementally, one page of commits at a time,
//! via repeated calls to [`CommitGraph::append_page`]. The policy that makes
//! lane numbers meaningful across those calls:
//!
//! 1. **Mainline convention**: a commit's *first* parent continues that
//!    commit's own lane (the same convention [`crate::commit::Commit`]'s
//!    consumers already use for "the" diff base of a merge commit). Only
//!    the second and later parents of a merge spawn new lanes.
//! 2. **Lowest free lane wins**: a new lane (for an additional parent, or
//!    for a commit whose hash no lane is currently awaiting — e.g. an
//!    additional branch head) always reuses the lowest-numbered free lane
//!    rather than growing without bound.
//! 3. **Rows are append-only and never renumbered**: once
//!    [`GraphRow`] is emitted for a commit, its `lane` and the identity of
//!    every edge it carries are final. A later page can only ever *resolve*
//!    a previously open edge (flip [`GraphEdge::resolved`] from `false` to
//!    `true` once the awaited parent is finally seen) — it never moves,
//!    removes, or reinterprets an already-emitted row. This is exactly what
//!    lets a caller preserve a selection by commit hash across an appended
//!    page (US-065 criterion 3): the row for a given hash is always at the
//!    same index with the same lane, no matter how many later pages load.
//! 4. **A missing parent is never invented a connection**: whenever a
//!    commit's parent hash is not found among the commits appended so far —
//!    because it is simply on a page not loaded yet, because history was
//!    fetched shallowly, or because a path/author filter excluded it — the
//!    edge to it is left `resolved: false` rather than being dropped
//!    silently or pointed at an unrelated row (US-065 criteria 1-2). The
//!    domain layer deliberately does not distinguish *why* the parent is
//!    absent; a caller that knows there is no more history to load (e.g.
//!    [`crate::error`]-free `has_more: false` at the port boundary) is what
//!    decides whether to present that as "load more" or as a shallow/root
//!    boundary — this module only guarantees the edge is never fabricated.

use crate::commit::{Commit, Decoration};
use crate::ids::CommitHash;

/// The identity and graph-relevant edges of one commit, decoupled from the
/// rest of [`Commit`] (author, subject, timestamps, ...) so a layout can be
/// computed from just what the graph needs (US-064 criterion 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub hash: CommitHash,
    pub parents: Vec<CommitHash>,
    pub decorations: Vec<Decoration>,
}

impl From<&Commit> for GraphCommit {
    fn from(commit: &Commit) -> Self {
        Self {
            hash: commit.hash.clone(),
            parents: commit.parents.clone(),
            decorations: commit.decorations.clone(),
        }
    }
}

/// One connection from a [`GraphRow`] to one of its commit's parents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    /// The lane this edge starts from — always the row's own [`GraphRow::lane`].
    pub from_lane: usize,
    /// The lane this edge leads to: the same lane when it is the first
    /// parent continuing the mainline, a different (new or converging)
    /// lane otherwise.
    pub to_lane: usize,
    /// The parent commit hash this edge names. Never invented: always one
    /// of the owning [`GraphRow`]'s commit's real parents.
    pub target: CommitHash,
    /// Whether `target` has been seen as some row's [`GraphRow::commit`] in
    /// this [`CommitGraph`] (this page or an earlier one). `false` means the
    /// edge currently leads nowhere resolvable yet — a continuation marker,
    /// never a wrong connection (US-065 criterion 1).
    pub resolved: bool,
}

/// One row of the graph: exactly one commit, the lane it occupies, and how
/// it connects to its parents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub commit: CommitHash,
    pub lane: usize,
    /// Echoes the [`GraphCommit`] this row was built from, so a renderer can
    /// show ref decorations without re-zipping against the original input.
    pub decorations: Vec<Decoration>,
    /// This row's edges to its parents, in the same order as the source
    /// commit's `parents` (empty for a root commit).
    pub edges: Vec<GraphEdge>,
    /// Lanes other than `lane` that simply pass straight through this row —
    /// neither this commit's own lane nor the destination of any of this
    /// row's edges — so a renderer can draw an unbroken vertical connector
    /// for them without attributing them to this commit (US-066 criterion
    /// 2: legible without relying on this row's own color).
    pub passthrough_lanes: Vec<usize>,
}

/// A lane still waiting for a commit hash to appear as some future row,
/// exposed read-only for introspection/tests (US-065 criterion 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLane {
    pub lane: usize,
    pub awaited: CommitHash,
}

/// Internal bookkeeping for one pending (not-yet-resolved) lane: the hash it
/// awaits, and every edge (identified by its row/edge index within
/// [`CommitGraph::rows`]) that should flip to `resolved: true` once that
/// hash is finally seen as a row's commit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingLane {
    hash: CommitHash,
    waiters: Vec<(usize, usize)>,
}

/// The commit graph accumulated so far, built by one or more calls to
/// [`Self::append_page`] (US-064, US-065). See the module documentation for
/// the lane stability policy this type guarantees across pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitGraph {
    rows: Vec<GraphRow>,
    open: Vec<Option<PendingLane>>,
    lane_count: usize,
}

impl Default for CommitGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitGraph {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            open: Vec::new(),
            lane_count: 0,
        }
    }

    /// Every row laid out so far, across every [`Self::append_page`] call,
    /// in the exact order commits were appended.
    pub fn rows(&self) -> &[GraphRow] {
        &self.rows
    }

    /// The widest lane index ever allocated, plus one — how many lane
    /// columns a renderer needs to reserve.
    pub fn lane_count(&self) -> usize {
        self.lane_count
    }

    /// Every lane still awaiting a commit hash that has not appeared in any
    /// page appended so far (US-065 criterion 1).
    pub fn open_lanes(&self) -> Vec<OpenLane> {
        self.open
            .iter()
            .enumerate()
            .filter_map(|(lane, slot)| {
                slot.as_ref().map(|pending| OpenLane {
                    lane,
                    awaited: pending.hash.clone(),
                })
            })
            .collect()
    }

    /// Whether some lane is still waiting for `hash` (US-065 criterion 1).
    pub fn is_open(&self, hash: &CommitHash) -> bool {
        self.open
            .iter()
            .any(|slot| matches!(slot, Some(pending) if &pending.hash == hash))
    }

    /// The index into [`Self::rows`] of the row for `hash`, stable across
    /// every subsequent [`Self::append_page`] call (US-065 criterion 3): a
    /// caller can hold onto this to preserve a selection across pagination.
    pub fn row_index_of(&self, hash: &CommitHash) -> Option<usize> {
        self.rows.iter().position(|row| &row.commit == hash)
    }

    fn allocate_lane(open: &mut Vec<Option<PendingLane>>) -> usize {
        match open.iter().position(Option::is_none) {
            Some(lane) => lane,
            None => {
                open.push(None);
                open.len() - 1
            }
        }
    }

    /// Lays out one page of commits, in the order given, appending to
    /// [`Self::rows`] and continuing any lanes a previous page left open
    /// (US-065). `commits` should be in the same topological/date order
    /// [`crate::commit::Commit`] history is normally read in (parents after
    /// children); a commit already emitted by an earlier call must never be
    /// passed again. Returns just this call's new rows — a caller only
    /// interested in the incremental delta (e.g. to render/send only what
    /// changed) never needs to re-scan [`Self::rows`] (US-067 criterion 2).
    pub fn append_page(&mut self, commits: &[GraphCommit]) -> &[GraphRow] {
        let start = self.rows.len();

        let mut open: Vec<Option<PendingLane>> = std::mem::take(&mut self.open);

        for graph_commit in commits {
            let commit_lane = match open
                .iter()
                .position(|slot| matches!(slot, Some(pending) if pending.hash == graph_commit.hash))
            {
                Some(lane) => {
                    if let Some(pending) = open[lane].take() {
                        for (row, edge) in pending.waiters {
                            self.rows[row].edges[edge].resolved = true;
                        }
                    }
                    lane
                }
                None => Self::allocate_lane(&mut open),
            };
            if open.len() <= commit_lane {
                open.resize_with(commit_lane + 1, || None);
            }

            let passthrough_lanes: Vec<usize> = open
                .iter()
                .enumerate()
                .filter_map(|(lane, slot)| (lane != commit_lane && slot.is_some()).then_some(lane))
                .collect();

            let row_index = self.rows.len();
            let mut edges = Vec::with_capacity(graph_commit.parents.len());
            for (i, parent) in graph_commit.parents.iter().enumerate() {
                let edge_index = edges.len();

                if let Some(existing_lane) = open
                    .iter()
                    .position(|slot| matches!(slot, Some(pending) if &pending.hash == parent))
                {
                    open[existing_lane]
                        .as_mut()
                        .expect("position() only matches Some slots")
                        .waiters
                        .push((row_index, edge_index));
                    edges.push(GraphEdge {
                        from_lane: commit_lane,
                        to_lane: existing_lane,
                        target: parent.clone(),
                        resolved: false,
                    });
                    continue;
                }

                let to_lane = if i == 0 {
                    commit_lane
                } else {
                    Self::allocate_lane(&mut open)
                };
                if open.len() <= to_lane {
                    open.resize_with(to_lane + 1, || None);
                }
                open[to_lane] = Some(PendingLane {
                    hash: parent.clone(),
                    waiters: vec![(row_index, edge_index)],
                });
                edges.push(GraphEdge {
                    from_lane: commit_lane,
                    to_lane,
                    target: parent.clone(),
                    resolved: false,
                });
            }

            self.lane_count = self.lane_count.max(open.len());
            self.rows.push(GraphRow {
                commit: graph_commit.hash.clone(),
                lane: commit_lane,
                decorations: graph_commit.decorations.clone(),
                edges,
                passthrough_lanes,
            });
        }

        self.open = open;
        &self.rows[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(seed: u8) -> CommitHash {
        CommitHash::new(format!("{:040x}", seed)).unwrap()
    }

    fn commit(seed: u8, parents: &[u8]) -> GraphCommit {
        GraphCommit {
            hash: hash(seed),
            parents: parents.iter().map(|p| hash(*p)).collect(),
            decorations: Vec::new(),
        }
    }

    // -- US-064: single-page layout over known DAGs --------------------

    #[test]
    fn a_linear_history_stays_on_a_single_stable_lane() {
        // c3 -> c2 -> c1 -> c0 (root), newest first as `git log` orders it.
        let commits = vec![commit(3, &[2]), commit(2, &[1]), commit(1, &[0]), commit(0, &[])];
        let mut graph = CommitGraph::new();
        graph.append_page(&commits);

        assert_eq!(graph.rows().len(), 4);
        assert!(graph.rows().iter().all(|row| row.lane == 0), "{:?}", graph.rows());
        assert!(graph.rows().iter().all(|row| row.edges.iter().all(|e| e.resolved)));
        assert!(graph.rows().last().unwrap().edges.is_empty(), "a root commit has no edges");
        assert!(graph.open_lanes().is_empty());
    }

    #[test]
    fn a_merge_commit_opens_a_second_lane_that_converges_back() {
        // m (parents a, b); a -> x; b -> x; x root. a/b both converge on x.
        let commits = vec![
            commit(0xAA, &[0xA1, 0xB1]), // merge
            commit(0xA1, &[0xC0]),
            commit(0xB1, &[0xC0]),
            commit(0xC0, &[]),
        ];
        let mut graph = CommitGraph::new();
        graph.append_page(&commits);

        let merge = &graph.rows()[0];
        assert_eq!(merge.lane, 0);
        assert_eq!(merge.edges.len(), 2);
        assert_eq!(merge.edges[0].to_lane, 0, "first parent continues the mainline lane");
        assert_eq!(merge.edges[1].to_lane, 1, "second parent spawns a new lane");

        let row_a = &graph.rows()[1];
        assert_eq!(row_a.lane, 0);
        assert_eq!(row_a.passthrough_lanes, vec![1], "lane 1 (awaiting b) passes through untouched");

        let row_b = &graph.rows()[2];
        assert_eq!(row_b.lane, 1);

        let row_x = &graph.rows()[3];
        assert_eq!(row_x.lane, 0, "the two converging lanes both awaited the same commit");
        assert!(row_x.edges.is_empty());

        // Both the merge's edges must now be resolved: the shared ancestor
        // was found, by both lanes, at the same row.
        assert!(graph.rows()[0].edges.iter().all(|e| e.resolved));
        assert!(graph.open_lanes().is_empty());
    }

    #[test]
    fn independent_branches_get_independent_lanes_and_never_cross_talk() {
        let commits = vec![
            commit(0xD2, &[0xD1]),
            commit(0xE2, &[0xE1]),
            commit(0xD1, &[]),
            commit(0xE1, &[]),
        ];
        let mut graph = CommitGraph::new();
        graph.append_page(&commits);

        assert_eq!(graph.lane_count(), 2);
        let lane_of = |seed: u8| graph.rows().iter().find(|r| r.commit == hash(seed)).unwrap().lane;
        assert_eq!(lane_of(0xD2), lane_of(0xD1), "one branch's lane stays consistent");
        assert_eq!(lane_of(0xE2), lane_of(0xE1), "the other branch's lane stays consistent");
        assert_ne!(lane_of(0xD2), lane_of(0xE2), "unrelated branches never share a lane");
        assert!(graph.rows().iter().all(|r| r.edges.iter().all(|e| e.resolved)));
    }

    #[test]
    fn a_root_commit_has_no_outgoing_edges_and_leaves_nothing_open() {
        let commits = vec![commit(1, &[])];
        let mut graph = CommitGraph::new();
        graph.append_page(&commits);

        assert!(graph.rows()[0].edges.is_empty());
        assert!(graph.open_lanes().is_empty());
    }

    #[test]
    fn layout_is_deterministic_for_the_same_input() {
        let commits = vec![
            commit(0xAA, &[0xA1, 0xB1]),
            commit(0xA1, &[0xC0]),
            commit(0xB1, &[0xC0]),
            commit(0xC0, &[]),
        ];
        let mut first = CommitGraph::new();
        first.append_page(&commits);
        let mut second = CommitGraph::new();
        second.append_page(&commits);

        assert_eq!(first.rows(), second.rows());
        assert_eq!(first.lane_count(), second.lane_count());
    }

    // -- US-065: continuity across pages ---------------------------------

    #[test]
    fn a_parent_outside_the_current_page_is_marked_unresolved_not_dropped() {
        let mut graph = CommitGraph::new();
        // Page 1 ends right after c2; c1 (its parent) has not loaded yet.
        graph.append_page(&[commit(2, &[1])]);

        let row = &graph.rows()[0];
        assert_eq!(row.edges.len(), 1);
        assert!(
            !row.edges[0].resolved,
            "a parent not yet loaded must be a continuation marker, never silently dropped"
        );
        assert!(graph.is_open(&hash(1)));
        assert!(
            graph.rows().iter().all(|r| r.commit != hash(1)),
            "an unloaded parent must never be fabricated as a row"
        );
    }

    #[test]
    fn appending_the_next_page_resolves_the_earlier_pages_continuation_edge() {
        let mut graph = CommitGraph::new();
        graph.append_page(&[commit(2, &[1])]);
        assert!(!graph.rows()[0].edges[0].resolved);

        graph.append_page(&[commit(1, &[0])]);

        assert!(
            graph.rows()[0].edges[0].resolved,
            "page 1's edge must resolve once its target is finally loaded"
        );
        assert_eq!(graph.rows()[1].commit, hash(1));
        assert!(!graph.is_open(&hash(1)));
    }

    #[test]
    fn lane_numbering_is_stable_across_a_page_boundary() {
        let mut graph = CommitGraph::new();
        graph.append_page(&[commit(2, &[1])]);
        let lane_before = graph.rows()[0].lane;
        let open_lane = graph.open_lanes()[0].lane;

        graph.append_page(&[commit(1, &[0])]);

        assert_eq!(graph.rows()[0].lane, lane_before, "an emitted row's lane never changes");
        assert_eq!(graph.rows()[1].lane, open_lane, "the continuing commit reuses the lane it was awaited on");
    }

    #[test]
    fn appending_a_page_preserves_the_row_index_of_an_earlier_selection() {
        let mut graph = CommitGraph::new();
        graph.append_page(&[commit(2, &[1]), commit(1, &[0])]);
        let selected = hash(1);
        let index_before = graph.row_index_of(&selected).unwrap();
        let lane_before = graph.rows()[index_before].lane;

        graph.append_page(&[commit(0, &[])]);

        assert_eq!(
            graph.row_index_of(&selected),
            Some(index_before),
            "a selection tracked by hash must resolve to the same row after appending a page"
        );
        assert_eq!(graph.rows()[index_before].lane, lane_before);
    }

    #[test]
    fn a_shallow_or_filtered_boundary_never_invents_a_connection() {
        // Simulates a shallow clone's boundary (or a filter that excludes
        // the real parent): the awaited hash never appears in any page.
        let mut graph = CommitGraph::new();
        graph.append_page(&[commit(1, &[0])]);

        assert!(!graph.rows()[0].edges[0].resolved);
        assert!(graph.is_open(&hash(0)));

        // A second, unrelated page loads (e.g. a different branch's
        // history) — it must not accidentally "resolve" the open lane by
        // matching an unrelated commit, nor fabricate a row for `hash(0)`.
        graph.append_page(&[commit(9, &[])]);

        assert!(!graph.rows()[0].edges[0].resolved, "an unrelated page must never resolve a dangling edge");
        assert!(graph.is_open(&hash(0)));
        assert!(graph.rows().iter().all(|r| r.commit != hash(0)));
    }

    #[test]
    fn multiple_pages_accumulate_rows_in_append_order() {
        let mut graph = CommitGraph::new();
        let first = graph.append_page(&[commit(3, &[2])]).to_vec();
        let second = graph.append_page(&[commit(2, &[1])]).to_vec();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(graph.rows().len(), 2);
        assert_eq!(graph.rows()[0].commit, hash(3));
        assert_eq!(graph.rows()[1].commit, hash(2));
    }
}
