//! The "Render" stage of SAD §18's event/update/render model (US-040,
//! US-043, US-046, US-047, US-048). Reads [`App`] but never mutates it.
//!
//! The layout follows `assets/mockups/gitsail_tui_mockup.png`: a left
//! chrome column (branding, a navigation mirror, the current branch and
//! working tree, the help strip), a wide center column (Commits above,
//! Diff/Blame below with the changed-file list nested inside it), a right
//! column (Repository facts, Branches, References), and a keycap strip
//! along the bottom. Colors, glyphs and their degraded forms all come from
//! [`crate::theme`]; nothing here resolves a color by hand.
//!
//! Only the *presentation* changed: focus still moves between the same five
//! [`Panel`]s in the same order, and every shortcut `docs/manual/tui.md`
//! documents resolves exactly as before — this module has no input path at
//! all.
//!
//! Every piece of text that originates from the repository (file/branch
//! names, diff/blame content) is passed through [`crate::sanitize`] before
//! reaching a widget (SAD §33) — this module is the render boundary that
//! rule applies at; `App` itself always holds the raw value.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::graph_view;
use gitsail_application::{
    CherryPickResult, MergeResult, PullOutcome, RebaseAction, RebaseResult, ResetMode, RevertResult,
};
use gitsail_domain::{
    BlameOrigin, BranchKind, Commit, ConflictSideContent, ConflictStage, Diff, DiffLineOrigin,
    GitTimestamp, TagKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{
    App, DiffViewMode, Panel, PatchApplyOutcome, PatchExportOutcome, ReferenceView, ViewPhase,
};
use crate::operation::{OperationKind, OperationState};
use crate::sanitize;
use crate::status_view::DiffScope;
use crate::theme::{self, DotKind, Icon, Role, Theme};

/// Below this width or height the three-column layout has no room left to
/// be legible; a single message replaces it instead (US-043 criterion 2:
/// "Resize adapta painéis e comunica largura mínima quando necessário").
///
/// Raised from 60x16 when the layout gained its chrome and detail columns:
/// the narrowest tier below still needs 24 + 26 columns of those plus ~28
/// for the commit list, and the keycap strip costs three rows on top of
/// what the two stacked center panels need. 78x20 fits inside the classic
/// 80x24 terminal with room to spare.
const MIN_WIDTH: u16 = 78;
const MIN_HEIGHT: u16 = 20;

/// Width of the left chrome column and the right detail column for a given
/// terminal width. Both stay visible at every valid size — the Branches and
/// References panels live in the right column and are focusable, so hiding
/// it would make two of the five panels unreachable — they only get
/// narrower, handing the difference to the center column.
fn column_widths(total: u16) -> (u16, u16) {
    if total >= 132 {
        (32, 34)
    } else if total >= 112 {
        (30, 32)
    } else if total >= 96 {
        (28, 30)
    } else {
        (24, 26)
    }
}

/// Everything a render pass needs beyond the frame itself: the state to
/// read, the resolved [`Theme`], and a single wall-clock sample.
///
/// The clock is sampled once here rather than per row so every relative
/// timestamp in one frame is measured against the same instant — two rows
/// a second apart on either side of a bucket boundary would otherwise
/// disagree about what "1 hour ago" means within the same screen.
struct Ui<'a> {
    app: &'a App,
    theme: Theme,
    now: i64,
}

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(frame, area);
        return;
    }

    let ui = Ui {
        app,
        theme: Theme::new(app.low_color()),
        now: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let (chrome_w, detail_w) = column_widths(area.width);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(chrome_w),
            Constraint::Min(24),
            Constraint::Length(detail_w),
        ])
        .split(outer[0]);

    // The Commits panel gets the larger share: it is the panel the mockup
    // leads with and the one whose rows are most often scrolled.
    let center = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(columns[1]);

    ui.render_chrome_column(frame, columns[0]);
    ui.render_commits_panel(frame, center[0]);
    ui.render_diff_panel(frame, center[1]);
    ui.render_detail_column(frame, columns[2]);
    ui.render_status_bar(frame, outer[1]);

    render_overlays(frame, area, app);
}

impl Ui<'_> {
    // -- shared building blocks ------------------------------------------

    /// A titled, rounded box. Focus is conveyed by the `»` title marker and
    /// a bold border *before* any color is added (US-043 criterion 3), so
    /// the focused panel is still identifiable with color off.
    fn boxed(
        &self,
        area: Rect,
        focused: bool,
        mut title: Vec<Span<'static>>,
        summary: Vec<Span<'static>>,
    ) -> Block<'static> {
        let border_style = if focused {
            let base = Style::default().add_modifier(Modifier::BOLD);
            if self.theme.low_color() {
                base
            } else {
                base.patch(self.theme.style(Role::Accent))
            }
        } else {
            self.theme.style(Role::Border)
        };
        let mut title_w = 0;
        if !title.is_empty() {
            title.insert(0, Span::raw(if focused { "»" } else { " " }));
            title.push(Span::raw(" "));
            title_w = theme::spans_width(&title);
        } else if focused {
            title.push(Span::raw("»"));
            title_w = 1;
        }

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(self.theme.border_type())
            .border_style(border_style);
        if !title.is_empty() {
            block = block.title_top(Line::from(title));
        }
        // Ratatui draws both titles on the same border row, so a summary
        // that no longer fits would overprint the title rather than wrap —
        // it is dropped instead, since the panel's name is the part that
        // must never become unreadable.
        let summary_w = theme::spans_width(&summary) + 2;
        if !summary.is_empty() && title_w + summary_w + 2 <= area.width as usize {
            let mut summary = summary;
            summary.insert(0, Span::raw(" "));
            summary.push(Span::raw(" "));
            block = block.title_top(Line::from(summary).right_aligned());
        }
        block
    }

    fn focused(&self, panel: Panel) -> bool {
        self.app.focus() == panel
    }

    fn icon_span(&self, icon: Icon, role: Role) -> Span<'static> {
        self.theme.span(self.theme.icon(icon), role)
    }

    /// The leading one-column gutter every selectable list carries, so the
    /// cursor position never depends on the highlight bar's color.
    fn cursor_cell(&self, selected: bool) -> Span<'static> {
        if selected {
            self.theme.span(self.theme.icon(Icon::Cursor), Role::Accent)
        } else {
            Span::raw(" ")
        }
    }

    /// Renders `items` as a list whose selected row is a solid, full-width
    /// bar, scrolled so the cursor stays on screen.
    fn render_rows(
        &self,
        frame: &mut Frame,
        area: Rect,
        rows: Vec<(Line<'static>, bool)>,
        cursor: usize,
    ) {
        let items: Vec<ListItem> = rows
            .into_iter()
            .map(|(line, selected)| {
                if selected {
                    ListItem::new(Line::from(theme::repaint(
                        line.spans,
                        self.theme.selection(),
                    )))
                    .style(self.theme.selection())
                } else {
                    ListItem::new(line)
                }
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(cursor));
        frame.render_stateful_widget(List::new(items), area, &mut state);
    }

    fn placeholder(&self, frame: &mut Frame, area: Rect, block: Block<'static>, label: &str) {
        let text = match self.app.view_phase() {
            ViewPhase::Loading => "Loading…".to_string(),
            ViewPhase::Empty => "Nothing to show — empty repository.".to_string(),
            ViewPhase::Error => "Unavailable — see the error in the left column.".to_string(),
            ViewPhase::Loaded => format!("{label} view not implemented yet."),
        };
        frame.render_widget(
            Paragraph::new(self.theme.span(text, Role::Muted))
                .wrap(Wrap { trim: true })
                .block(block),
            area,
        );
    }

    /// Commits are paged (`GRAPH_PAGE_SIZE` at a time), so the number the
    /// header shows is how many are *loaded*, never a repository total
    /// nobody has counted — `42+` says another page is still out there.
    fn commit_count_label(&self) -> String {
        let loaded = self.app.graph_commits().len();
        if self.app.graph_has_more() {
            format!("{loaded}+")
        } else {
            loaded.to_string()
        }
    }

    // -- left chrome column ----------------------------------------------

    /// The whole left column sits inside one frame, like the mockup's
    /// sidebar: brand, navigation, the branch/working-tree card and the
    /// help row are all contained by it, rather than floating on the canvas
    /// beside panels that are not.
    fn render_chrome_column(&self, frame: &mut Frame, area: Rect) {
        let frame_block = self.boxed(area, false, Vec::new(), Vec::new());
        let inner = frame_block.inner(area);
        frame.render_widget(frame_block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Brand art and the branch/working-tree card are the first things
        // to go when the terminal is short: the navigation mirror and the
        // help row both carry information the others repeat.
        let full_brand = inner.height >= 26;
        let brand_h = if full_brand { 6 } else { 2 };
        let show_card = inner.height >= 14;
        let card_h = if show_card { 6 } else { 0 };
        // A rule plus the keys, not a nested box: one more border inside an
        // already-framed column is noise rather than structure.
        let help_h = 2;
        let nav_h = inner
            .height
            .saturating_sub(brand_h + card_h + help_h)
            .min(NAV_SLOTS);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(brand_h),
                Constraint::Length(nav_h),
                Constraint::Min(0),
                Constraint::Length(card_h),
                Constraint::Length(help_h),
            ])
            .split(inner);

        self.render_brand(frame, rows[0], full_brand);
        self.render_nav(frame, rows[1]);
        if show_card {
            self.render_branch_card(frame, rows[3]);
        }
        self.render_help_strip(frame, rows[4]);
    }

    /// The mockup's mascot and wordmark. A terminal cannot show the
    /// illustration, so this is a deliberate stand-in: a sail drawn with
    /// Box Drawing/Block Elements glyphs, the wordmark beside it, and the
    /// tagline underneath — with a pure-ASCII sail for `--ascii`.
    fn render_brand(&self, frame: &mut Frame, area: Rect, full: bool) {
        let w = area.width as usize;
        let ascii = self.theme.low_color();
        let mut lines: Vec<Line<'static>> = Vec::new();

        if full {
            let sail = if ascii {
                ["   /|", " /==|  GitSail", "/===|"]
            } else {
                ["   ╱▌", " ╱══▌  GitSail", "╱═══▌"]
            };
            // The mark is a compact, centred block: the waterline is only as
            // wide as the boat and wordmark it sits under. Run across the
            // whole column it reads as a divider rule, not as water.
            let mark_w = sail.iter().map(|l| theme::width(l)).max().unwrap_or(0);
            for line in sail {
                lines.push(Line::from(
                    self.theme.span(center(line, mark_w, w), Role::Accent),
                ));
            }
            let wave = if ascii { "~" } else { "≈" };
            lines.push(Line::from(
                self.theme
                    .span(center(&wave.repeat(mark_w), mark_w, w), Role::Muted),
            ));
            let tagline = theme::clip(self.theme, "Navigate your Git history.", w);
            let tagline_w = theme::width(&tagline);
            lines.push(Line::from(
                self.theme.span(center(&tagline, tagline_w, w), Role::Muted),
            ));
            lines.push(Line::from(self.theme.span("─".repeat(w), Role::Border)));
        } else {
            let mark = format!("{} GitSail", if ascii { "/|" } else { "╱▌" });
            let mark_w = theme::width(&mark);
            lines.push(Line::from(
                self.theme.span(center(&mark, mark_w, w), Role::Accent),
            ));
            lines.push(Line::from(self.theme.span("─".repeat(w), Role::Border)));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    /// The navigation menu. Every row maps to something that actually
    /// exists in this app — a focusable [`Panel`], or a sub-view of one —
    /// and the highlighted row is derived from the current focus rather
    /// than being state of its own, so `Tab`/`t`/`b` keep their exact
    /// documented meanings and this column simply mirrors them.
    ///
    /// The mockup's "Settings" row has no counterpart (the TUI has no
    /// settings screen; keybindings are a config file), so it is left out
    /// rather than rendered as a dead entry.
    fn render_nav(&self, frame: &mut Frame, area: Rect) {
        if area.height == 0 {
            return;
        }
        let items = self.nav_items();
        let active = items.iter().position(|i| i.active).unwrap_or(0);
        let w = area.width as usize;

        let rows: Vec<(Line<'static>, bool)> = items
            .iter()
            .map(|item| {
                let left = vec![
                    self.cursor_cell(item.active),
                    self.icon_span(item.icon, Role::Accent),
                    Span::raw(" "),
                    self.theme.span(item.label, Role::Text),
                ];
                let right = match &item.count {
                    Some(count) => vec![self.theme.span(count.clone(), Role::Muted)],
                    None => Vec::new(),
                };
                (theme::lay_row(self.theme, w, left, right), item.active)
            })
            .collect();

        self.render_rows(frame, area, rows, active);
    }

    fn nav_items(&self) -> Vec<NavItem> {
        let app = self.app;
        let focus = app.focus();
        let refs = app.reference_view();
        let diff_mode = app.diff_view_mode();
        let on_refs = |view: ReferenceView| focus == Panel::References && refs == view;

        vec![
            NavItem {
                icon: Icon::Commits,
                label: "Commits",
                count: Some(self.commit_count_label()),
                active: focus == Panel::Graph,
            },
            NavItem {
                icon: Icon::Branches,
                label: "Branches",
                count: Some(app.branches().len().to_string()),
                active: focus == Panel::Sidebar,
            },
            NavItem {
                icon: Icon::Changes,
                label: "Changes",
                count: Some(app.status_entries().len().to_string()),
                active: focus == Panel::Details,
            },
            NavItem {
                icon: Icon::Diff,
                label: "Diff",
                count: None,
                active: focus == Panel::Diff && diff_mode == DiffViewMode::Diff,
            },
            NavItem {
                icon: Icon::Blame,
                label: "Blame",
                count: None,
                active: focus == Panel::Diff && diff_mode == DiffViewMode::Blame,
            },
            NavItem {
                icon: Icon::Stashes,
                label: "Stashes",
                count: Some(app.stashes().len().to_string()),
                active: on_refs(ReferenceView::Stash),
            },
            NavItem {
                icon: Icon::Remotes,
                label: "Remotes",
                count: Some(app.remotes().len().to_string()),
                active: on_refs(ReferenceView::Remotes),
            },
            NavItem {
                icon: Icon::Tags,
                label: "Tags",
                count: Some(app.tags().len().to_string()),
                active: on_refs(ReferenceView::Tags),
            },
            NavItem {
                icon: Icon::Reflog,
                label: "Reflog",
                count: Some(app.reflog().len().to_string()),
                active: on_refs(ReferenceView::Reflog),
            },
        ]
    }

    /// The mockup's "Current branch / Working tree" card. Loading, empty
    /// and error phases are all named in place here (this is the one spot
    /// that always shows *why* the rest of the screen is blank), so the
    /// error text stays visible at every terminal width.
    fn render_branch_card(&self, frame: &mut Frame, area: Rect) {
        let block = self.boxed(
            area,
            false,
            vec![self.theme.span("Current branch", Role::Muted)],
            Vec::new(),
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 {
            return;
        }
        let w = inner.width as usize;

        let (branch_text, branch_role) = match self.app.view_phase() {
            ViewPhase::Loading => ("loading…".to_string(), Role::Muted),
            ViewPhase::Error => ("unavailable".to_string(), Role::Danger),
            ViewPhase::Empty | ViewPhase::Loaded => self
                .app
                .session()
                .and_then(|s| s.repository().current_branch.as_ref())
                .map(|b| (sanitize::safe_line(b.as_str()), Role::Accent))
                .unwrap_or_else(|| ("(detached HEAD)".to_string(), Role::Warn)),
        };

        let changed = self
            .app
            .session()
            .and_then(|s| s.status())
            .map(|s| s.files.len())
            .unwrap_or(0);
        let clean = changed == 0;

        let branch_line = theme::lay_row(
            self.theme,
            w,
            vec![
                self.icon_span(Icon::Branches, Role::Muted),
                Span::raw(" "),
                self.theme.span(branch_text, branch_role),
            ],
            vec![self.theme.dot(if clean {
                DotKind::Current
            } else {
                DotKind::Local
            })],
        );

        // Phrased so the word "status" and the clean/dirty verdict are both
        // literal text: the dot beside the branch above is the decoration,
        // never the only statement of the state.
        let status_line = match self.app.view_phase() {
            ViewPhase::Loading => Line::from(self.theme.span("status: loading…", Role::Muted)),
            // An unborn HEAD has no working tree to be clean or dirty
            // about yet; saying "Clean" here would be a claim about a
            // comparison that has no left-hand side.
            ViewPhase::Empty => Line::from(self.theme.span("(no commits yet)", Role::Muted)),
            ViewPhase::Error => {
                let message = self
                    .app
                    .status_error()
                    .or(self.app.discovery_error())
                    .map(|e| e.message())
                    .unwrap_or("unknown error");
                Line::from(self.theme.span(
                    theme::clip(
                        self.theme,
                        &format!("error — {}", sanitize::safe_line(message)),
                        w,
                    ),
                    Role::Danger,
                ))
            }
            ViewPhase::Loaded if clean => Line::from(vec![
                self.icon_span(Icon::Clean, Role::Ok),
                Span::raw(" "),
                self.theme.span("Clean", Role::Ok),
            ]),
            ViewPhase::Loaded => Line::from(vec![
                self.icon_span(Icon::Dirty, Role::Warn),
                Span::raw(" "),
                self.theme.span(format!("{changed} changed"), Role::Warn),
            ]),
        };

        let lines = vec![
            branch_line,
            Line::from(self.theme.span("─".repeat(w), Role::Border)),
            Line::from(self.theme.span("Working tree", Role::Muted)),
            status_line,
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_help_strip(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let w = area.width as usize;
        let lines = vec![
            Line::from(self.theme.span("─".repeat(w), Role::Border)),
            Line::from(vec![
                Span::styled(" ? ", self.theme.selection()),
                Span::raw(" Help   "),
                Span::styled(" q ", self.theme.selection()),
                Span::raw(" Quit"),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    }

    // -- center: Commits --------------------------------------------------

    fn render_commits_panel(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let mut title = vec![
            self.icon_span(Icon::Commits, Role::Accent),
            Span::raw(" "),
            self.theme.span("Commits", Role::Strong),
        ];
        // The *submitted* filter belongs in the title (US-045 criterion 2);
        // the box being typed into gets its own line below, so the two
        // never compete for the same row.
        if let Some(filter) = app.active_commit_filter() {
            title.push(self.theme.span(
                format!(" (filter: {})", sanitize::safe_line(filter)),
                Role::Muted,
            ));
        }
        let summary = vec![self.theme.span(
            format!(
                "{} commit{}",
                self.commit_count_label(),
                if app.graph_commits().len() == 1 && !app.graph_has_more() {
                    ""
                } else {
                    "s"
                }
            ),
            Role::Muted,
        )];
        let block = self.boxed(area, self.focused(Panel::Graph), title, summary);

        if app.view_phase() != ViewPhase::Loaded {
            self.placeholder(frame, area, block, "Commits");
            return;
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let content = match app.commit_search() {
            Some(query) => {
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(inner);
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        self.theme.span("/", Role::Accent),
                        self.theme.span(sanitize::safe_line(query), Role::Text),
                        self.theme.span("▏", Role::Accent),
                    ])),
                    split[0],
                );
                split[1]
            }
            None => inner,
        };

        if let Some(error) = app.graph_error() {
            frame.render_widget(
                Paragraph::new(
                    self.theme
                        .span(sanitize::safe_line(error.message()), Role::Danger),
                )
                .wrap(Wrap { trim: true }),
                content,
            );
            return;
        }

        let graph_rows = app.commit_graph().rows();
        if graph_rows.is_empty() {
            let text = if app.graph_loading() {
                "Loading history…"
            } else if app.active_commit_filter().is_some() {
                "No commits match this search."
            } else {
                "No commits yet."
            };
            frame.render_widget(Paragraph::new(self.theme.span(text, Role::Muted)), content);
            return;
        }

        let w = content.width as usize;
        // "2 hours ago" only earns its width once the subject still has
        // room to say something; below that the column falls back to "2h".
        let long_time = w >= 58;
        let lane_count = app.commit_graph().lane_count();
        let focused = self.focused(Panel::Graph);

        let mut rows: Vec<(Line<'static>, bool)> = graph_rows
            .iter()
            .zip(app.graph_commits().iter())
            .enumerate()
            .map(|(i, (row, commit))| {
                let selected = i == app.graph_cursor();
                let mut left = vec![self.cursor_cell(selected)];
                for (lane, glyph) in graph_view::lane_glyphs(row, commit, lane_count)
                    .into_iter()
                    .enumerate()
                {
                    left.push(Span::styled(glyph.to_string(), self.theme.lane_color(lane)));
                    left.push(Span::raw(" "));
                }
                left.push(
                    self.theme
                        .span(sanitize::safe_line(commit.short_hash.as_str()), Role::Hash),
                );
                left.push(Span::raw(" "));
                let decorations = graph_view::decoration_labels(row);
                if !decorations.is_empty() {
                    left.push(self.theme.span(
                        format!("({}) ", sanitize::safe_line(&decorations.join(", "))),
                        Role::Accent,
                    ));
                }
                left.push(
                    self.theme
                        .span(sanitize::safe_line(&commit.subject), Role::Text),
                );
                if graph_view::has_unresolved_edge(row) {
                    left.push(self.theme.span(" (continues…)", Role::Muted));
                }

                let right = vec![
                    self.theme.span(
                        theme::relative_time(
                            self.now,
                            commit.author_date.seconds_since_epoch,
                            long_time,
                        ),
                        Role::Muted,
                    ),
                    Span::raw(" "),
                    self.theme
                        .author_initial(&commit.author.name, &commit.author.email),
                ];
                (
                    theme::lay_row(self.theme, w, left, right),
                    selected && focused,
                )
            })
            .collect();

        if app.graph_loading() {
            rows.push((
                Line::from(self.theme.span("  Loading more…", Role::Muted)),
                false,
            ));
        }

        self.render_rows(frame, content, rows, app.graph_cursor());
    }

    // -- center: Diff, with the changed-file list nested inside ------------

    /// Renders the Diff panel: the mockup's bottom-center box, whose header
    /// carries the changed-file summary and whose body nests the Changes
    /// list ([`Panel::Details`]) above the hunks. Shows the diff or the
    /// blame of the currently selected file depending on [`DiffViewMode`]
    /// (US-046 criteria 2, 3).
    ///
    /// The mockup titles this box after a *commit* (`Diff (a1b2c3d)`). No
    /// commit diff exists in `App` — the diff loaded here is always the
    /// selected working-tree/index file's — so the title names that file
    /// instead of implying data the app does not have.
    fn render_diff_panel(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let blame = app.diff_view_mode() == DiffViewMode::Blame;
        let mut title = vec![
            self.icon_span(if blame { Icon::Blame } else { Icon::Diff }, Role::Accent),
            Span::raw(" "),
            self.theme
                .span(if blame { "Blame" } else { "Diff" }, Role::Strong),
        ];
        if let Some(entry) = app.selected_file() {
            title.push(self.theme.span(
                format!(" ({})", sanitize::safe_path(&entry.path)),
                Role::Muted,
            ));
        }
        let block = self.boxed(
            area,
            self.focused(Panel::Diff),
            title,
            self.diff_summary_spans(),
        );

        if app.view_phase() != ViewPhase::Loaded {
            self.placeholder(frame, area, block, "Diff");
            return;
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // The changed-file list takes what it needs up to half the box, so
        // a long status never squeezes the hunks out of existence.
        let entries = app.status_entries().len().max(1) as u16;
        let changes_h = (entries + 2).clamp(3, (inner.height / 2).max(3));
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(changes_h), Constraint::Min(0)])
            .split(inner);

        self.render_changes_box(frame, split[0]);
        self.render_diff_body(frame, split[1]);
    }

    fn diff_summary_spans(&self) -> Vec<Span<'static>> {
        let Some(diff) = self.app.diff() else {
            return Vec::new();
        };
        let (files, added, removed) = diff_stats(diff);
        vec![
            self.theme.span(
                format!("{files} file{} changed ", if files == 1 { "" } else { "s" }),
                Role::Muted,
            ),
            self.theme.span(format!("+{added}"), Role::Ok),
            Span::raw(" "),
            self.theme.span(format!("-{removed}"), Role::Danger),
        ]
    }

    /// The mockup's nested file box. This is [`Panel::Details`] — the
    /// status list whose rows `s` stages/unstages and `Enter` opens — so it
    /// carries its own focus marker independently of the Diff box around
    /// it.
    fn render_changes_box(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let entries = app.status_entries();
        let block = self.boxed(
            area,
            self.focused(Panel::Details),
            vec![
                self.icon_span(Icon::Changes, Role::Accent),
                Span::raw(" "),
                self.theme.span("Changes", Role::Strong),
            ],
            if entries.is_empty() {
                Vec::new()
            } else {
                vec![self
                    .theme
                    .span(format!("{} changed", entries.len()), Role::Muted)]
            },
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        if entries.is_empty() {
            frame.render_widget(
                Paragraph::new(self.theme.span("No changes.", Role::Muted)),
                inner,
            );
            return;
        }

        let w = inner.width as usize;
        let focused = self.focused(Panel::Details);
        let rows: Vec<(Line<'static>, bool)> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == app.status_cursor();
                // `S`/`W` stay literal: index and worktree scope is the one
                // distinction US-046 criterion 1 turns on, and it must not
                // depend on the color it is also given.
                let (tag, role) = match entry.scope {
                    DiffScope::Staged => ("S", Role::Ok),
                    DiffScope::Worktree => ("W", Role::Warn),
                };
                let left = vec![
                    self.cursor_cell(selected),
                    self.theme.span(tag, role),
                    Span::raw(" "),
                    self.theme
                        .span(sanitize::safe_path(&entry.path), Role::Text),
                ];
                let right = vec![self
                    .theme
                    .span(format!("{:?}", entry.change_type), Role::Muted)];
                (
                    theme::lay_row(self.theme, w, left, right),
                    selected && focused,
                )
            })
            .collect();
        self.render_rows(frame, inner, rows, app.status_cursor());
    }

    fn render_diff_body(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let lines: Vec<Line<'static>> = if app.selected_file().is_none() {
            vec![Line::from(self.theme.span(
                "No file selected — press Enter on a Changes entry.",
                Role::Muted,
            ))]
        } else {
            match app.diff_view_mode() {
                DiffViewMode::Diff => self.diff_lines(area.width as usize),
                DiffViewMode::Blame => self.blame_lines(area.width as usize),
            }
        };

        let scroll = match app.diff_view_mode() {
            DiffViewMode::Blame => app.blame_scroll(),
            DiffViewMode::Diff => 0,
        };

        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            area,
        );
    }

    /// Content lines for the diff sub-view: binary/truncated banners
    /// (US-046 criterion 2) come before any hunk, never in place of one — a
    /// truncated file's withheld hunks are never confused with "no
    /// changes".
    ///
    /// Each line is laid out as the mockup draws it: old line number, new
    /// line number, the `+`/`-`/space sign column, then the code. The sign
    /// column is what distinguishes the three kinds; the red/green
    /// backgrounds are layered on top of it, never instead of it.
    fn diff_lines(&self, width: usize) -> Vec<Line<'static>> {
        let app = self.app;
        // A patch-export result (US-029 criterion 1) is shown regardless of
        // which of the states below applies — even "Loading diff…" or an
        // error — since it reports on the *previous* action, not on what is
        // currently loading; an `if let ... return` chain would otherwise
        // hide it whenever one of those early states applies.
        let banner = app.patch_export().map(|o| self.patch_export_banner(o));
        let apply_banner = app
            .patch_apply_outcome()
            .map(|o| self.patch_apply_banner(o));

        let mut lines = Vec::new();
        if let Some(err) = app.diff_error() {
            lines.push(Line::from(
                self.theme
                    .span(sanitize::safe_line(err.message()), Role::Danger),
            ));
        } else if let Some(diff) = app.diff() {
            if let Some(file) = diff.files.first() {
                if file.is_binary {
                    lines.push(Line::from(self.theme.span("[binary file]", Role::Warn)));
                }
                if file.truncated {
                    lines.push(Line::from(self.theme.span(
                        "[diff truncated — content withheld, not empty]",
                        Role::Warn,
                    )));
                }
                for (i, hunk) in file.hunks.iter().enumerate() {
                    let header = format!(
                        "@@ -{},{} +{},{} @@",
                        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                    );
                    let selected = i == app.diff_hunk_cursor();
                    let style = if selected {
                        self.theme.selection()
                    } else {
                        self.theme.style(Role::Accent)
                    };
                    // Ruled out to the right edge so a hunk boundary reads
                    // as a separator rather than as another line of code.
                    let rule = if self.theme.low_color() { "-" } else { "─" };
                    let pad = width.saturating_sub(theme::width(&header) + 1);
                    lines.push(Line::from(vec![
                        Span::styled(header, style),
                        Span::styled(
                            format!(" {}", rule.repeat(pad)),
                            self.theme.style(Role::Border),
                        ),
                    ]));

                    let mut old_no = hunk.old_start;
                    let mut new_no = hunk.new_start;
                    for line in &hunk.lines {
                        let (sign, role, old_cell, new_cell) = match line.origin {
                            DiffLineOrigin::Addition => {
                                let n = new_no;
                                new_no += 1;
                                ("+", Role::DiffAdd, None, Some(n))
                            }
                            DiffLineOrigin::Deletion => {
                                let n = old_no;
                                old_no += 1;
                                ("-", Role::DiffRemove, Some(n), None)
                            }
                            DiffLineOrigin::Context => {
                                let (o, n) = (old_no, new_no);
                                old_no += 1;
                                new_no += 1;
                                (" ", Role::Text, Some(o), Some(n))
                            }
                        };
                        lines.push(self.diff_line(
                            width,
                            sign,
                            role,
                            old_cell,
                            new_cell,
                            &line.content,
                        ));
                    }
                }
                if lines.is_empty() {
                    lines.push(Line::from(self.theme.span("No hunks.", Role::Muted)));
                }
            } else {
                lines.push(Line::from(
                    self.theme.span("No changes for this file.", Role::Muted),
                ));
            }
        } else {
            lines.push(Line::from(self.theme.span("Loading diff…", Role::Muted)));
        }

        if let Some(banner) = apply_banner {
            lines.insert(0, banner);
        }
        if let Some(banner) = banner {
            lines.insert(0, banner);
        }
        lines
    }

    fn diff_line(
        &self,
        width: usize,
        sign: &'static str,
        role: Role,
        old_no: Option<u32>,
        new_no: Option<u32>,
        content: &str,
    ) -> Line<'static> {
        let gutter = self.theme.style(Role::Muted);
        let base = self.theme.style(role);
        let mut spans = vec![
            Span::styled(
                old_no
                    .map(|n| format!("{n:>4}"))
                    .unwrap_or_else(|| "    ".into()),
                gutter,
            ),
            Span::styled(
                new_no
                    .map(|n| format!(" {n:>4} "))
                    .unwrap_or_else(|| "      ".into()),
                gutter,
            ),
            Span::styled(
                sign,
                base.patch(Style::default().add_modifier(Modifier::BOLD)),
            ),
            Span::styled(" ", base),
        ];
        let used = theme::spans_width(&spans);
        let code = sanitize::safe_line(content);
        let room = width.saturating_sub(used);
        let (mut code_spans, code_w) =
            theme::truncate_spans(self.theme, self.highlight(&code, base), room);
        // An added/removed line's background has to reach the right edge or
        // the block reads as ragged noise rather than a band.
        if role != Role::Text {
            code_spans.push(Span::styled(" ".repeat(room - code_w), base));
        }
        spans.extend(code_spans);
        Line::from(spans)
    }

    /// A deliberately small, language-agnostic pass over a line of code:
    /// comments, string literals, numbers and a handful of near-universal
    /// keywords. Enough for the mockup's "lightly colored code" without
    /// adding a syntax-highlighting dependency (and a grammar set to keep
    /// current) to a Git client.
    ///
    /// `text` must already be sanitized — this only ever splits it, never
    /// reinterprets it.
    fn highlight(&self, text: &str, base: Style) -> Vec<Span<'static>> {
        if self.theme.low_color() {
            return vec![Span::styled(text.to_string(), base)];
        }
        let trimmed = text.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("--") {
            return vec![Span::styled(
                text.to_string(),
                base.patch(self.theme.style(Role::Muted)),
            )];
        }

        let mut spans = Vec::new();
        let mut buffer = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' || c == '\'' {
                flush_word(&mut buffer, &mut spans, base, self.theme);
                let mut literal = String::from(c);
                for n in chars.by_ref() {
                    literal.push(n);
                    if n == c {
                        break;
                    }
                }
                spans.push(Span::styled(
                    literal,
                    base.patch(self.theme.style(Role::Ok)),
                ));
            } else if c.is_alphanumeric() || c == '_' {
                buffer.push(c);
            } else {
                flush_word(&mut buffer, &mut spans, base, self.theme);
                spans.push(Span::styled(c.to_string(), base));
            }
        }
        flush_word(&mut buffer, &mut spans, base, self.theme);
        spans
    }

    /// Renders the outcome of the last `y` (export patch) press as a
    /// single, bold banner line (US-029 criterion 1: origin/scope,
    /// criterion 3: clipboard vs. file-fallback outcome — both stated
    /// explicitly, never left for the person to infer).
    fn patch_export_banner(&self, outcome: &PatchExportOutcome) -> Line<'static> {
        let (text, role) = match outcome {
            PatchExportOutcome::Copied {
                scope,
                file_count,
                incomplete,
            } => (
                format!(
                    "Patch copied to clipboard — {scope} ({file_count} file{}){}",
                    if *file_count == 1 { "" } else { "s" },
                    if *incomplete {
                        " [incomplete: binary/truncated content skipped]"
                    } else {
                        ""
                    }
                ),
                Role::Ok,
            ),
            PatchExportOutcome::SavedToFile {
                scope,
                path,
                incomplete,
                reason,
            } => (
                format!(
                    "Clipboard unavailable ({reason}) — patch for {scope} saved to {}{}",
                    path.display(),
                    if *incomplete {
                        " [incomplete: binary/truncated content skipped]"
                    } else {
                        ""
                    }
                ),
                Role::Warn,
            ),
            PatchExportOutcome::Failed { reason } => {
                (format!("Patch export failed: {reason}"), Role::Danger)
            }
            PatchExportOutcome::Empty => (
                "Nothing to export — no content hunks in the current diff".to_string(),
                Role::Muted,
            ),
        };
        Line::from(Span::styled(
            sanitize::safe_line(&text),
            self.theme
                .style(role)
                .patch(Style::default().add_modifier(Modifier::BOLD)),
        ))
    }

    /// Renders the outcome of the last `Y` (apply patch) press as a single,
    /// bold banner line (T-163/US-030 criteria 1-3), mirroring
    /// [`Ui::patch_export_banner`]'s own convention.
    fn patch_apply_banner(&self, outcome: &PatchApplyOutcome) -> Line<'static> {
        let (text, role) = match outcome {
            PatchApplyOutcome::ClipboardEmpty { reason } => (
                format!("Nothing to apply from the clipboard ({reason})"),
                Role::Muted,
            ),
            PatchApplyOutcome::Rejected { reason } => {
                (format!("Patch rejected: {reason}"), Role::Danger)
            }
            PatchApplyOutcome::Failed { reason } => {
                (format!("Patch apply failed: {reason}"), Role::Danger)
            }
            PatchApplyOutcome::Applied { affected_files } => (
                format!(
                    "Patch applied — {} file{} changed",
                    affected_files.len(),
                    if affected_files.len() == 1 { "" } else { "s" }
                ),
                Role::Ok,
            ),
        };
        Line::from(Span::styled(
            sanitize::safe_line(&text),
            self.theme
                .style(role)
                .patch(Style::default().add_modifier(Modifier::BOLD)),
        ))
    }

    /// Content lines for the blame sub-view (US-046 criterion 3): line
    /// number, author, and commit — [`BlameOrigin::Local`] is shown as
    /// `"local"` rather than a synthetic commit, so uncommitted content is
    /// never mistaken for a real attribution (US-033).
    fn blame_lines(&self, width: usize) -> Vec<Line<'static>> {
        let app = self.app;
        if let Some(err) = app.blame_error() {
            return vec![Line::from(
                self.theme
                    .span(sanitize::safe_line(err.message()), Role::Danger),
            )];
        }
        let Some(blame) = app.blame() else {
            return vec![Line::from(self.theme.span("Loading blame…", Role::Muted))];
        };
        if blame.lines.is_empty() {
            return vec![Line::from(
                self.theme.span("No lines to blame.", Role::Muted),
            )];
        }
        blame
            .lines
            .iter()
            .map(|line| {
                let attribution = match line.origin {
                    BlameOrigin::Local => "local   ".to_string(),
                    BlameOrigin::Committed => line.commit.to_short(8).as_str().to_string(),
                };
                let mut spans = vec![
                    self.theme.span(attribution, Role::Hash),
                    Span::raw(" "),
                    self.theme.span(
                        format!(
                            "{:<16}",
                            theme::clip(self.theme, &sanitize::safe_line(&line.author.name), 16)
                        ),
                        Role::Muted,
                    ),
                    self.theme
                        .span(format!("{:>5} ", line.final_line), Role::Muted),
                ];
                let used = theme::spans_width(&spans);
                let code = sanitize::safe_line(&line.content);
                let (code_spans, _) = theme::truncate_spans(
                    self.theme,
                    self.highlight(&code, Style::default()),
                    width.saturating_sub(used),
                );
                spans.extend(code_spans);
                Line::from(spans)
            })
            .collect()
    }

    // -- right detail column ----------------------------------------------

    fn render_detail_column(&self, frame: &mut Frame, area: Rect) {
        // Repository is a fixed fact sheet, References sizes to its own
        // content, and Branches — the focusable list people scroll — takes
        // whatever is left.
        let repo_h = if area.height >= 26 { 11 } else { 6 };
        let refs_h = ((self.app.reference_len() as u16) + 2).clamp(5, 12);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(repo_h),
                Constraint::Min(5),
                Constraint::Length(refs_h.min(area.height.saturating_sub(repo_h + 5))),
            ])
            .split(area);

        self.render_repository_box(frame, rows[0]);
        self.render_branches_panel(frame, rows[1]);
        self.render_references_panel(frame, rows[2]);
    }

    fn render_repository_box(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let block = self.boxed(
            area,
            false,
            vec![
                self.icon_span(Icon::Repository, Role::Accent),
                Span::raw(" "),
                self.theme.span("Repository", Role::Strong),
            ],
            Vec::new(),
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let w = inner.width as usize;

        let name = app
            .repo_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| app.repo_path().display().to_string());
        let branch = app
            .session()
            .and_then(|s| s.repository().current_branch.as_ref())
            .map(|b| sanitize::safe_line(b.as_str()))
            .unwrap_or_else(|| "(detached)".to_string());

        let mut lines = vec![
            theme::lay_row(
                self.theme,
                w,
                vec![
                    self.icon_span(Icon::Repository, Role::Muted),
                    Span::raw(" "),
                    self.theme.span(sanitize::safe_line(&name), Role::Strong),
                ],
                vec![
                    self.icon_span(Icon::Branches, Role::Muted),
                    Span::raw(" "),
                    self.theme
                        .span(theme::clip(self.theme, &branch, w / 2), Role::Accent),
                ],
            ),
            Line::from(self.theme.span(
                theme::clip(self.theme, &sanitize::safe_path(app.repo_path()), w),
                Role::Muted,
            )),
        ];

        if inner.height > 3 {
            lines.push(Line::from(self.theme.span("─".repeat(w), Role::Border)));
            let facts: [(Icon, &str, String); 5] = [
                (Icon::Commits, "Commits", self.commit_count_label()),
                (Icon::Branches, "Branches", app.branches().len().to_string()),
                (Icon::Stashes, "Stashes", app.stashes().len().to_string()),
                (Icon::Remotes, "Remotes", app.remotes().len().to_string()),
                (Icon::Tags, "Tags", app.tags().len().to_string()),
            ];
            for (icon, label, value) in facts {
                lines.push(theme::lay_row(
                    self.theme,
                    w,
                    vec![
                        self.icon_span(icon, Role::Muted),
                        Span::raw(" "),
                        self.theme.span(label, Role::Text),
                    ],
                    vec![self.theme.span(value, Role::Strong)],
                ));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// The Branches panel ([`Panel::Sidebar`]) — the mockup's right-hand
    /// branch list. US-048 criterion 1: local/remote and the current branch
    /// must all be distinguishable from this list alone, which is why the
    /// dot's *shape* differs per kind and the remote's name is still
    /// spelled out in brackets.
    ///
    /// The mockup shows a per-branch age; a `Branch` carries no date, so
    /// the right-hand column shows what it does carry — ahead/behind
    /// against its upstream, or the target's short hash.
    fn render_branches_panel(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let branches = app.filtered_branches();
        let block = self.boxed(
            area,
            self.focused(Panel::Sidebar),
            vec![
                self.icon_span(Icon::Branches, Role::Accent),
                Span::raw(" "),
                self.theme.span("Branches", Role::Strong),
            ],
            vec![self.theme.span(branches.len().to_string(), Role::Muted)],
        );

        if app.view_phase() != ViewPhase::Loaded {
            self.placeholder(frame, area, block, "Branches");
            return;
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let content = match app.search() {
            Some(query) => {
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(inner);
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        self.theme.span("/", Role::Accent),
                        self.theme.span(sanitize::safe_line(query), Role::Text),
                        self.theme.span("▏", Role::Accent),
                    ])),
                    split[0],
                );
                split[1]
            }
            None => inner,
        };

        if branches.is_empty() {
            frame.render_widget(
                Paragraph::new(self.theme.span("No branches.", Role::Muted)),
                content,
            );
            return;
        }

        let w = content.width as usize;
        let focused = self.focused(Panel::Sidebar);
        let rows: Vec<(Line<'static>, bool)> = branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let selected = i == app.sidebar_cursor();
                let kind = if branch.is_current {
                    DotKind::Current
                } else {
                    match branch.kind {
                        BranchKind::Local => DotKind::Local,
                        BranchKind::Remote { .. } => DotKind::Remote,
                    }
                };
                let mut left = vec![
                    self.cursor_cell(selected),
                    self.theme.dot(kind),
                    Span::raw(" "),
                    self.theme.span(
                        sanitize::safe_line(branch.name.as_str()),
                        if branch.is_current {
                            Role::Accent
                        } else {
                            Role::Text
                        },
                    ),
                ];
                if let BranchKind::Remote { remote } = &branch.kind {
                    left.push(
                        self.theme
                            .span(format!(" [{}]", sanitize::safe_line(remote)), Role::Muted),
                    );
                }

                let right = if branch.ahead > 0 || branch.behind > 0 {
                    vec![
                        self.theme.span(
                            format!("{}{}", self.theme.icon(Icon::Ahead), branch.ahead),
                            Role::Ok,
                        ),
                        self.theme.span(
                            format!("{}{}", self.theme.icon(Icon::Behind), branch.behind),
                            Role::Warn,
                        ),
                    ]
                } else {
                    vec![self
                        .theme
                        .span(branch.target.to_short(7).as_str().to_string(), Role::Hash)]
                };
                (
                    theme::lay_row(self.theme, w, left, right),
                    selected && focused,
                )
            })
            .collect();
        self.render_rows(frame, content, rows, app.sidebar_cursor());
    }

    /// The References panel: whichever of Tags/Remotes/Stash/Reflog
    /// [`ReferenceView`] currently selects (US-050 criterion 1), with an
    /// explicit empty state per sub-view (criterion 3) rather than a single
    /// generic "nothing here" for all four.
    ///
    /// This is where the mockup's "Recent Activity" box lands: same shape
    /// (a colored dot, a label, a right-aligned age), but filled with the
    /// real, already-loaded references instead of a second copy of the
    /// commit list.
    fn render_references_panel(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        let view = app.reference_view();
        let icon = match view {
            ReferenceView::Tags => Icon::Tags,
            ReferenceView::Remotes => Icon::Remotes,
            ReferenceView::Stash => Icon::Stashes,
            ReferenceView::Reflog => Icon::Reflog,
        };
        let block = self.boxed(
            area,
            self.focused(Panel::References),
            vec![
                self.icon_span(icon, Role::Accent),
                Span::raw(" "),
                self.theme.span(view.title(), Role::Strong),
                self.theme.span(
                    if area.width >= 30 { "  (t cycles)" } else { "" },
                    Role::Muted,
                ),
            ],
            vec![self
                .theme
                .span(app.reference_len().to_string(), Role::Muted)],
        );

        if app.view_phase() != ViewPhase::Loaded {
            self.placeholder(frame, area, block, "References");
            return;
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let w = inner.width as usize;

        let empty = match view {
            ReferenceView::Tags => "No tags.",
            ReferenceView::Remotes => "No remotes configured.",
            ReferenceView::Stash => "No stash entries.",
            ReferenceView::Reflog => "No reflog entries.",
        };
        if app.reference_len() == 0 {
            frame.render_widget(Paragraph::new(self.theme.span(empty, Role::Muted)), inner);
            return;
        }

        let focused = self.focused(Panel::References);
        let rows: Vec<(Line<'static>, bool)> = self
            .reference_rows(w)
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                let selected = i == app.reference_cursor();
                (line, selected && focused)
            })
            .collect();
        self.render_rows(frame, inner, rows, app.reference_cursor());
    }

    fn reference_rows(&self, w: usize) -> Vec<Line<'static>> {
        let app = self.app;
        let cursor = app.reference_cursor();
        let dot = |i: usize, theme: Theme| {
            theme.dot(if i == cursor {
                DotKind::Current
            } else {
                DotKind::Local
            })
        };
        match app.reference_view() {
            ReferenceView::Tags => app
                .tags()
                .iter()
                .enumerate()
                .map(|(i, tag)| {
                    // Abbreviated because the column is ~4 cells wide; the
                    // details overlay (`Enter`) spells the kind out in full.
                    let (kind, date) = match &tag.kind {
                        TagKind::Lightweight => ("lw", None),
                        TagKind::Annotated { date, .. } => ("ann", Some(date)),
                    };
                    theme::lay_row(
                        self.theme,
                        w,
                        vec![
                            self.cursor_cell(i == cursor),
                            dot(i, self.theme),
                            Span::raw(" "),
                            self.theme.span(sanitize::safe_line(&tag.name), Role::Text),
                            self.theme
                                .span(format!(" {}", tag.target.to_short(8).as_str()), Role::Hash),
                        ],
                        vec![self.theme.span(
                            match date {
                                Some(d) => {
                                    theme::relative_time(self.now, d.seconds_since_epoch, false)
                                }
                                None => kind.to_string(),
                            },
                            Role::Muted,
                        )],
                    )
                })
                .collect(),
            // `Remote`'s URLs render already-redacted via their own
            // `Display` (SAD §11, §28) — never the raw credential-bearing
            // string.
            ReferenceView::Remotes => app
                .remotes()
                .iter()
                .enumerate()
                .map(|(i, remote)| {
                    theme::lay_row(
                        self.theme,
                        w,
                        vec![
                            self.cursor_cell(i == cursor),
                            dot(i, self.theme),
                            Span::raw(" "),
                            self.theme
                                .span(sanitize::safe_line(&remote.name), Role::Text),
                            Span::raw(" "),
                            self.theme.span(
                                sanitize::safe_line(&remote.fetch_url.to_string()),
                                Role::Muted,
                            ),
                        ],
                        Vec::new(),
                    )
                })
                .collect(),
            ReferenceView::Stash => app
                .stashes()
                .iter()
                .enumerate()
                .map(|(i, stash)| {
                    theme::lay_row(
                        self.theme,
                        w,
                        vec![
                            self.cursor_cell(i == cursor),
                            dot(i, self.theme),
                            Span::raw(" "),
                            self.theme
                                .span(format!("stash@{{{}}}", stash.index), Role::Hash),
                            Span::raw(" "),
                            self.theme
                                .span(sanitize::safe_line(&stash.message), Role::Text),
                        ],
                        vec![self.theme.span(
                            theme::relative_time(self.now, stash.date.seconds_since_epoch, false),
                            Role::Muted,
                        )],
                    )
                })
                .collect(),
            // T-241/US-089 criterion 1: reference (`HEAD@{n}`), hash, and
            // message are all shown; criterion 3: an entry whose object no
            // longer exists is marked `[missing]` right in the list, never
            // silently hidden.
            ReferenceView::Reflog => app
                .reflog()
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let mut left = vec![
                        self.cursor_cell(i == cursor),
                        dot(i, self.theme),
                        Span::raw(" "),
                        self.theme.span(entry.selector("HEAD"), Role::Hash),
                        Span::raw(" "),
                        self.theme
                            .span(sanitize::safe_line(&entry.message), Role::Text),
                    ];
                    if !entry.is_available() {
                        left.push(self.theme.span(" [missing]", Role::Danger));
                    }
                    theme::lay_row(
                        self.theme,
                        w,
                        left,
                        vec![self.theme.span(
                            theme::relative_time(self.now, entry.date.seconds_since_epoch, false),
                            Role::Muted,
                        )],
                    )
                })
                .collect(),
        }
    }

    // -- bottom keycap strip ------------------------------------------------

    /// The mockup's bottom bar: each key in its own little box, then a
    /// label. When a mode-specific hint applies (an overlay, a text input,
    /// a pending conflict) the middle row carries that hint instead — the
    /// keycaps describe the Normal context and would be actively wrong
    /// while another context owns the keyboard.
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let app = self.app;
        if let Some(hint) = contextual_hint(app) {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);
            frame.render_widget(
                Paragraph::new(self.theme.span(hint, Role::Accent)).wrap(Wrap { trim: true }),
                rows[1],
            );
            return;
        }

        let chips: [(&str, &str); 6] = [
            (self.theme.icon(Icon::UpDown), "Navigate"),
            (self.theme.icon(Icon::Enter), "Open"),
            ("Tab", "Focus"),
            ("/", "Search"),
            ("?", "Help"),
            ("q", "Quit"),
        ];
        let (tl, tr, bl, br, h, v) = if self.theme.low_color() {
            ("+", "+", "+", "+", "-", "|")
        } else {
            ("╭", "╮", "╰", "╯", "─", "│")
        };

        let mut top: Vec<Span<'static>> = Vec::new();
        let mut mid: Vec<Span<'static>> = Vec::new();
        let mut bot: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        let cap = area.width as usize;
        for (key, label) in chips {
            // Each chip occupies `│key│ label  ` on the middle row; the two
            // border rows must be padded to that *same* display width, or
            // every box after the first drifts left of the key it frames.
            // Widths are display cells, never bytes or `char`s — `↑↓` is two
            // cells and `⏎` one.
            let kw = theme::width(key);
            let trailer = theme::width(label) + 3;
            let cost = kw + 2 + trailer;
            if used + cost > cap {
                break;
            }
            used += cost;
            let chip = self.theme.key_chip();
            let border = self.theme.style(Role::Border);
            let pad = " ".repeat(trailer);
            top.push(Span::styled(
                format!("{tl}{}{tr}{pad}", h.repeat(kw)),
                border,
            ));
            mid.push(Span::styled(v, border));
            mid.push(Span::styled(key.to_string(), chip));
            mid.push(Span::styled(v, border));
            mid.push(self.theme.span(format!(" {label}  "), Role::Text));
            bot.push(Span::styled(
                format!("{bl}{}{br}{pad}", h.repeat(kw)),
                border,
            ));
        }

        // The wordmark tails the strip exactly as the mockup does, but only
        // when it does not start eating into the keycaps.
        let brand = format!(
            "{} GitSail — Navigate your Git history.",
            if self.theme.low_color() {
                "/|"
            } else {
                "╱▌"
            }
        );
        if used + theme::width(&brand) + 2 <= cap {
            mid.push(Span::raw(" ".repeat(cap - used - theme::width(&brand))));
            mid.push(self.theme.span(brand, Role::Accent));
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
        frame.render_widget(Paragraph::new(Line::from(top)), rows[0]);
        frame.render_widget(Paragraph::new(Line::from(mid)), rows[1]);
        frame.render_widget(Paragraph::new(Line::from(bot)), rows[2]);
    }
}

/// Pads `text` (whose display width the caller already knows) so it sits
/// centred in `total` columns. Used only by the brand block — the one part
/// of this layout that is decorative rather than tabular.
fn center(text: &str, text_width: usize, total: usize) -> String {
    format!("{}{text}", " ".repeat(total.saturating_sub(text_width) / 2))
}

struct NavItem {
    icon: Icon,
    label: &'static str,
    count: Option<String>,
    active: bool,
}

/// How many navigation rows [`Ui::nav_items`] can produce. Kept next to the
/// chrome-column height arithmetic that has to reserve room for them.
const NAV_SLOTS: u16 = 9;

/// Appends `buffer` to `spans`, tinting it if it is one of the keywords the
/// light diff highlighter recognizes. Free function rather than a method so
/// [`Ui::highlight`] can call it while holding a `&mut` borrow of both.
fn flush_word(buffer: &mut String, spans: &mut Vec<Span<'static>>, base: Style, theme: Theme) {
    if buffer.is_empty() {
        return;
    }
    let word = std::mem::take(buffer);
    let style = if KEYWORDS.contains(&word.as_str()) {
        base.patch(theme.style(Role::Accent))
    } else if word.chars().all(|c| c.is_ascii_digit()) {
        base.patch(theme.style(Role::Warn))
    } else {
        base
    };
    spans.push(Span::styled(word, style));
}

/// A deliberately small, cross-language keyword set — the diff viewer is
/// not a code editor, and a wrong guess here costs nothing but a missing
/// tint.
const KEYWORDS: [&str; 44] = [
    "as", "async", "await", "bool", "break", "class", "const", "continue", "crate", "def",
    "default", "dyn", "else", "enum", "export", "false", "fn", "for", "from", "function", "if",
    "impl", "import", "in", "int", "let", "loop", "match", "mod", "move", "mut", "pub", "return",
    "self", "static", "struct", "trait", "true", "type", "use", "var", "void", "where", "while",
];

fn diff_stats(diff: &Diff) -> (usize, usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for file in &diff.files {
        for hunk in &file.hunks {
            for line in &hunk.lines {
                match line.origin {
                    DiffLineOrigin::Addition => added += 1,
                    DiffLineOrigin::Deletion => removed += 1,
                    DiffLineOrigin::Context => {}
                }
            }
        }
    }
    (diff.files.len(), added, removed)
}

/// The mode-specific hint for whatever context currently owns the keyboard,
/// or `None` in the Normal context (where the keycap strip applies).
///
/// Mirrors [`crate::app::App::input_context`]'s own precedence exactly: the
/// hint must describe the context that will actually receive the next key.
fn contextual_hint(app: &App) -> Option<String> {
    // The operation overlay is modal (`App::input_context`), so while it is
    // up these are the *only* keys that do anything — saying so here is what
    // keeps US-042 criterion 3 ("nenhuma ação escondida") honest for it, and
    // the strip below would otherwise keep advertising Navigate/Open/Focus/
    // Search to someone whose keyboard those keys no longer reach (T-267).
    match app.operation() {
        OperationState::Confirming(_) => {
            return Some("Enter confirms · Esc/q cancels · nothing has changed yet".to_string())
        }
        OperationState::InProgress(_) => {
            return Some("Working… · already started, so it cannot be cancelled".to_string())
        }
        OperationState::Succeeded(_) | OperationState::Failed(_, _) => {
            return Some("Esc/Enter/q dismisses".to_string())
        }
        OperationState::Idle => {}
    }

    let hint = if app.commit_details_open() {
        "Esc/q closes commit details"
    } else if app.reference_details_open() {
        "Esc/q closes reference details"
    } else if app.reflog_details_open() {
        "Esc/q closes reflog entry details"
    } else if app.amend_open() {
        "Type amend message · Enter confirms · Esc discards"
    } else if app.conflicts_open() {
        "j/k select · Enter inspects · r resolves · o/t take ours/theirs · c continue · a abort · s skip · Esc closes"
    } else if app.rebase_plan_reword_input().is_some() {
        "Type the new message · Enter confirms · Esc cancels"
    } else if app.rebase_plan_open() {
        "j/k select · J/K move entry · a cycle action · Enter confirms plan · Esc closes"
    } else if app.in_progress_operation().has_conflicts() {
        return Some(format!(
            "{} conflicted file(s) — press 'M' to resolve them",
            app.in_progress_operation().conflicted_files().len()
        ));
    } else if app.commit_search().is_some() {
        "Type message/author:/branch:/hash · Enter searches · Esc cancels"
    } else if app.search().is_some() {
        "Type to filter · Enter/Esc close search"
    } else if app.branch_input().is_some() {
        "Type branch name · Enter confirms · Esc cancels"
    } else if app.commit_message().is_some() {
        "Type commit message · Enter confirms · Esc discards"
    } else {
        // The completed operation reports itself here instead of holding the
        // screen with a popup nobody could dismiss (T-267). Lowest priority
        // on purpose: anything the person is still *doing* — resolving
        // conflicts above, most of all — matters more than what already
        // finished.
        let kind = app.last_operation_outcome()?;
        let label = sanitize::safe_line(&kind.completion_label());
        return Some(match operation_outcome_detail(app, kind) {
            Some(detail) => format!("Done — {label}: {detail} · Esc clears"),
            None => format!("Done — {label} · Esc clears"),
        });
    };
    Some(hint.to_string())
}

/// The shared frame for every floating overlay: the same rounded border and
/// spaced title the panels use, so a popup reads as part of the same
/// surface rather than as a differently styled widget.
fn overlay_block(theme: Theme, title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
        .border_style(theme.style(Role::Accent))
        .title_top(Line::from(vec![
            Span::raw(" "),
            theme.span(title.to_string(), Role::Strong),
            Span::raw(" "),
        ]))
}

/// Renders whichever mode-specific overlay is active, topmost first: the
/// commit composer and the operation confirmation/progress popup are
/// mutually exclusive in practice (the keymap only ever routes input to one
/// active mode at a time — see [`crate::app::App::input_context`]), and the
/// contextual help always draws last so it is visible over anything else,
/// matching [`crate::app::App`]'s dismiss priority.
fn render_overlays(frame: &mut Frame, area: Rect, app: &App) {
    if app.commit_details_open() {
        render_commit_details(frame, area, app);
    } else if app.reference_details_open() {
        render_reference_details(frame, area, app);
    } else if app.reflog_details_open() {
        render_reflog_details(frame, area, app);
    } else if app.amend_open() {
        render_amend_overlay(frame, area, app);
    } else if app.commit_message().is_some() {
        render_commit_composer(frame, area, app);
    } else if !app.operation().is_idle() {
        render_operation_overlay(frame, area, app);
    } else if app.conflicts_open() {
        render_conflicts_overlay(frame, area, app);
    } else if app.rebase_plan_reword_input().is_some() {
        render_rebase_plan_reword(frame, area, app);
    } else if app.rebase_plan_open() {
        render_rebase_plan_overlay(frame, area, app);
    } else if app.reset_mode_open() {
        render_reset_mode_overlay(frame, area, app);
    } else if app.branch_input().is_some() {
        render_branch_name_prompt(frame, area, app);
    } else if app.sync_error().is_some() {
        render_sync_error(frame, area, app);
    } else if app.forge_link_error().is_some() {
        render_forge_link_error(frame, area, app);
    }

    if app.help_visible() {
        render_help(frame, area, Theme::new(app.low_color()));
    }
}

/// Shows the commit-details overlay (US-045 criterion 3): full hash,
/// author (name, email, date), committer too when it differs from the
/// author, and the complete message (subject plus body) — every field
/// [`gitsail_application::GetCommit`] would also return, read directly off
/// [`App::selected_graph_commit`] (see that method's doc for why no second
/// fetch is issued).
fn render_commit_details(frame: &mut Frame, area: Rect, app: &App) {
    let Some(commit) = app.selected_graph_commit() else {
        return;
    };
    let popup = centered_rect(80, 70, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(commit_details_lines(commit))
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Commit Details")),
        popup,
    );
}

fn commit_details_lines(commit: &Commit) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("commit {}", commit.hash.as_str())),
        Line::from(format!(
            "Author:      {} <{}>",
            sanitize::safe_line(&commit.author.name),
            sanitize::safe_line(&commit.author.email)
        )),
        Line::from(format!(
            "AuthorDate:  {}",
            format_timestamp(&commit.author_date)
        )),
    ];
    // A commit's author and committer differ whenever a rebase, cherry-pick,
    // or `am` recorded someone else as having committed it — showing the
    // committer only in that case keeps the common (self-authored) case
    // uncluttered while never hiding the distinction when it matters.
    if commit.committer != commit.author {
        lines.push(Line::from(format!(
            "Committer:   {} <{}>",
            sanitize::safe_line(&commit.committer.name),
            sanitize::safe_line(&commit.committer.email)
        )));
        lines.push(Line::from(format!(
            "CommitDate:  {}",
            format_timestamp(&commit.commit_date)
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(sanitize::safe_line(&commit.subject)));
    if !commit.body.trim().is_empty() {
        lines.push(Line::from(""));
        for body_line in commit.body.lines() {
            lines.push(Line::from(sanitize::safe_line(body_line)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Esc/q closes"));
    lines
}

/// Formats a [`GitTimestamp`] as a calendar date/time in the commit's own
/// recorded offset (T-251/US-109 criterion 1's format-alignment pass:
/// before this, this was the workspace's one remaining raw-epoch-integer
/// date display — `apps/vscode/src/blameFormat.ts::formatGitTimestamp` and
/// `apps/desktop`'s DTOs already expect/produce a calendar date wherever
/// they render one at all — see `docs/architecture/preferences-matrix.md`
/// for the full audit). Still zero-dependency (no `chrono`/`time` crate
/// anywhere in this workspace): shifts the epoch seconds by the offset and
/// reads the civil (Gregorian) date back off the result with the same
/// integer `civil_from_days` algorithm `blameFormat.ts`'s own doc
/// describes in prose — never the host machine's local timezone, so the
/// same commit renders identically regardless of where the TUI runs.
fn format_timestamp(ts: &GitTimestamp) -> String {
    let shifted = ts.seconds_since_epoch + i64::from(ts.utc_offset_minutes) * 60;
    let days = shifted.div_euclid(86_400);
    let seconds_of_day = shifted.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;

    let sign = if ts.utc_offset_minutes >= 0 { '+' } else { '-' };
    let offset = ts.utc_offset_minutes.unsigned_abs();
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} ({sign}{:02}:{:02})",
        offset / 60,
        offset % 60
    )
}

/// Howard Hinnant's `civil_from_days`: converts a day count relative to the
/// 1970-01-01 epoch (as `i64::div_euclid(86_400)` on a shifted Unix
/// timestamp produces) into a proleptic-Gregorian `(year, month, day)`,
/// `month`/`day` both 1-based. Pure integer arithmetic, valid for every
/// `i64` input — no dependency on any date/time crate.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let mp = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// Shows the reference-details overlay for the entry currently under the
/// References panel's cursor (US-050 criterion 2: selecting an item shows
/// its target/message and every metadata field available for that kind —
/// an annotated tag's message/tagger/date, a lightweight tag's bare target,
/// a remote's fetch/push URLs, or a stash entry's commit/message/date).
/// Mirrors [`render_commit_details`]: reads data already loaded for the
/// panel, never issuing a fresh read.
fn render_reference_details(frame: &mut Frame, area: Rect, app: &App) {
    let index = app.reference_cursor();
    let lines: Vec<Line<'static>> = match app.reference_view() {
        ReferenceView::Tags => match app.tags().get(index) {
            Some(tag) => {
                let mut lines = vec![
                    Line::from(format!("tag {}", sanitize::safe_line(&tag.name))),
                    Line::from(format!("target: {}", tag.target.as_str())),
                ];
                match &tag.kind {
                    TagKind::Lightweight => lines.push(Line::from("kind: lightweight")),
                    TagKind::Annotated {
                        message,
                        tagger,
                        date,
                    } => {
                        lines.push(Line::from("kind: annotated"));
                        lines.push(Line::from(format!(
                            "tagger: {} <{}>",
                            sanitize::safe_line(&tagger.name),
                            sanitize::safe_line(&tagger.email)
                        )));
                        lines.push(Line::from(format!("date: {}", format_timestamp(date))));
                        lines.push(Line::from(""));
                        for line in message.lines() {
                            lines.push(Line::from(sanitize::safe_line(line)));
                        }
                    }
                }
                lines
            }
            None => return,
        },
        ReferenceView::Remotes => match app.remotes().get(index) {
            Some(remote) => vec![
                Line::from(format!("remote {}", sanitize::safe_line(&remote.name))),
                Line::from(format!(
                    "fetch: {}",
                    sanitize::safe_line(&remote.fetch_url.to_string())
                )),
                Line::from(format!(
                    "push:  {}",
                    sanitize::safe_line(&remote.push_url.to_string())
                )),
            ],
            None => return,
        },
        ReferenceView::Stash => match app.stashes().get(index) {
            Some(stash) => vec![
                Line::from(format!("stash@{{{}}}", stash.index)),
                Line::from(format!("commit: {}", stash.commit.as_str())),
                Line::from(format!("date: {}", format_timestamp(&stash.date))),
                Line::from(""),
                Line::from(sanitize::safe_line(&stash.message)),
            ],
            None => return,
        },
        // Selecting a reflog entry opens `render_reflog_details` instead
        // (via `reflog_details_open`, never `reference_details_open`) — see
        // `crate::app::App::activate`'s `Panel::References` arm. This
        // overlay never actually renders for this sub-view; the arm exists
        // only to keep this match exhaustive.
        ReferenceView::Reflog => return,
    };
    let mut lines = lines;
    lines.push(Line::from(""));
    lines.push(Line::from("Esc/q closes"));

    let popup = centered_rect(70, 60, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(
                Theme::new(app.low_color()),
                "Reference Details",
            )),
        popup,
    );
}

/// Shows the reflog-entry commit-details overlay (T-241/US-089 criterion
/// 2), reusing [`commit_details_lines`] for a loaded object — the exact
/// same fields [`render_commit_details`] shows for a Graph panel commit,
/// never a bespoke, thinner rendering. While loading, or when the entry's
/// object no longer exists (criterion 3), that state is shown explicitly
/// instead.
fn render_reflog_details(frame: &mut Frame, area: Rect, app: &App) {
    let Some(entry) = app.reflog().get(app.reference_cursor()) else {
        return;
    };

    let mut lines = vec![Line::from(format!(
        "{} — {}",
        entry.selector("HEAD"),
        sanitize::safe_line(&entry.message)
    ))];
    lines.push(Line::from(""));

    match app.reflog_details_commit() {
        Some(commit) => lines.extend(commit_details_lines(commit)),
        None => match app.reflog_details_error() {
            Some(error) => {
                lines.push(Line::from(sanitize::safe_line(error.message())));
                lines.push(Line::from(""));
                lines.push(Line::from("Esc/q closes"));
            }
            None => lines.push(Line::from("Loading commit details…")),
        },
    }

    let popup = centered_rect(80, 70, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Reflog Entry")),
        popup,
    );
}

/// Shows a browser-launch failure from [`crate::action::Action::RequestOpenForgeLink`]
/// (T-243/US-101) — this is about the local OS process spawn failing, never
/// about Git itself, which is exactly why it is its own banner rather than
/// folded into [`render_sync_error`].
fn render_forge_link_error(frame: &mut Frame, area: Rect, app: &App) {
    let Some(error) = app.forge_link_error() else {
        return;
    };
    let mut lines = vec![Line::from(sanitize::safe_line(error.message()))];
    if let Some(remediation) = error.remediation() {
        lines.push(Line::from(""));
        lines.push(Line::from(sanitize::safe_line(remediation)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Esc dismisses"));

    let popup = centered_rect(60, 40, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(
                Theme::new(app.low_color()),
                "Open in Browser",
            )),
        popup,
    );
}

/// Shows a remote/branch resolution failure from a fetch/pull/push request
/// (US-049 criterion 1) — e.g. no remote configured, or an ambiguous choice
/// this app deliberately refuses to guess. Distinct from
/// [`render_operation_overlay`]: nothing was ever confirmed or dispatched
/// here, so there is no operation to show progress for.
fn render_sync_error(frame: &mut Frame, area: Rect, app: &App) {
    let Some(error) = app.sync_error() else {
        return;
    };
    let mut lines = vec![Line::from(sanitize::safe_line(error.message()))];
    if let Some(remediation) = error.remediation() {
        lines.push(Line::from(""));
        lines.push(Line::from(sanitize::safe_line(remediation)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Esc dismisses"));

    let popup = centered_rect(60, 40, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Remote Sync")),
        popup,
    );
}

/// The last operation's own, per-kind outcome detail: "fast-forwarded to
/// abc1234" rather than a bare "Done" (US-049 criterion 1; US-079 criteria
/// 2/3; US-083 criterion 3; US-086 criterion 3; US-087 criterion 2 — every
/// one of those asks for the distinct outcomes to be named explicitly).
///
/// Shared by the operation overlay and by [`contextual_hint`]'s status-bar
/// report, which is what keeps those distinctions visible now that a
/// successful operation closes its own overlay (T-267) instead of holding
/// the screen until dismissed.
fn operation_outcome_detail(app: &App, kind: &OperationKind) -> Option<String> {
    // US-049 criterion 1: a pull's exact outcome — already up to date, or
    // fast-forwarded to a specific commit — is shown explicitly rather than
    // a bare "Done", so it is never mistaken for a merge/rebase result.
    if matches!(kind, OperationKind::Pull { .. }) {
        if let Some(outcome) = app.last_pull_outcome() {
            let text = match outcome {
                PullOutcome::AlreadyUpToDate => "already up to date".to_string(),
                PullOutcome::FastForwarded { new_head } => {
                    format!("fast-forwarded to {}", new_head.to_short(8).as_str())
                }
            };
            return Some(text);
        }
    }

    // T-231/US-079 criterion 2/3: fast-forward, a new merge commit, and a
    // conflict are always three distinct, explicit lines — a conflict is
    // never left to be inferred from a bare "Done", and never silently
    // treated the same as either of the other two outcomes.
    if matches!(kind, OperationKind::Merge { .. }) {
        if let Some(outcome) = app.last_merge_result() {
            let text = match outcome {
                MergeResult::FastForwarded { new_head } => {
                    format!("fast-forwarded to {}", new_head.to_short(8).as_str())
                }
                MergeResult::MergeCommitCreated { hash } => {
                    format!("merge commit {} created", hash.to_short(8).as_str())
                }
                MergeResult::Conflict { files } => format!(
                    "CONFLICT — {} file{} need resolution (press 'M' to resolve)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            return Some(text);
        }
    }

    // T-235/US-083 criterion 3: completion and conflict are always two
    // distinct, explicit lines for a rebase too, mirroring the merge block
    // above exactly. Also covers T-236/US-084's `ExecuteRebasePlan`: both
    // report through the same `RebaseResult`, so the same two outcomes
    // apply unchanged (only the confirmation prompt's own target label
    // above distinguishes "plain rebase" from "interactive plan").
    if matches!(
        kind,
        OperationKind::Rebase { .. } | OperationKind::ExecuteRebasePlan { .. }
    ) {
        if let Some(outcome) = app.last_rebase_result() {
            let text = match outcome {
                RebaseResult::Completed { new_head } => {
                    format!("rebased onto {}", new_head.to_short(8).as_str())
                }
                RebaseResult::Conflict { files } => format!(
                    "CONFLICT — {} file{} need resolution (press 'M' to resolve)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            return Some(text);
        }
    }

    // T-238/US-086 criterion 3: applying, a conflict, and an empty "already
    // applied" result are always three distinct, explicit lines, mirroring
    // the merge/rebase blocks above exactly.
    if matches!(kind, OperationKind::CherryPick { .. }) {
        if let Some(outcome) = app.last_cherry_pick_result() {
            let text = match outcome {
                CherryPickResult::Applied { hash } => {
                    format!("applied as {}", hash.to_short(8).as_str())
                }
                CherryPickResult::Conflict { files } => format!(
                    "CONFLICT — {} file{} need resolution (press 'M' to resolve)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
                CherryPickResult::Empty => {
                    "EMPTY — already applied on the current branch (press 'M' to skip/abort)"
                        .to_string()
                }
            };
            return Some(text);
        }
    }

    // T-239/US-087 criterion 2: completion and conflict are always two
    // distinct, explicit lines, mirroring `CherryPick` above.
    if matches!(kind, OperationKind::Revert { .. }) {
        if let Some(outcome) = app.last_revert_result() {
            let text = match outcome {
                RevertResult::Applied { hash } => {
                    format!("reverted as {}", hash.to_short(8).as_str())
                }
                RevertResult::Conflict { files } => format!(
                    "CONFLICT — {} file{} need resolution (press 'M' to resolve)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            return Some(text);
        }
    }

    None
}

/// Shows a pending/running/finished mutation (US-047, US-048): what it
/// targets, its SAD §20 risk tier, and — for `SwitchBranch`/`DeleteBranch`
/// — the branch's current target as the "ref de origem" criterion 2 asks
/// for. A failure never implies anything was discarded (criterion 3).
fn render_operation_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let (kind, status_line, error_line) = match app.operation() {
        OperationState::Confirming(kind) => (kind, "Enter confirms · Esc/q cancels", None),
        OperationState::InProgress(kind) => (kind, "Working…", None),
        OperationState::Succeeded(kind) => (kind, "Done — Esc dismisses", None),
        OperationState::Failed(kind, err) => (
            kind,
            "Nothing was changed — Esc/Enter/q dismisses",
            Some(sanitize::safe_line(err.message())),
        ),
        OperationState::Idle => return,
    };

    let mut lines = vec![
        Line::from(sanitize::safe_line(&kind.prompt_label())),
        Line::from(format!("risk: {:?}", kind.risk())),
    ];

    let origin_name = match kind {
        OperationKind::SwitchBranch { target } => Some(target.as_str()),
        OperationKind::DeleteBranch { name, .. } => Some(name.as_str()),
        _ => None,
    };
    if let Some(name) = origin_name {
        if let Some(branch) = app.branches().iter().find(|b| b.name.as_str() == name) {
            lines.push(Line::from(format!(
                "ref: {}",
                branch.target.to_short(8).as_str()
            )));
        }
    }

    if let Some(detail) = operation_outcome_detail(app, kind) {
        lines.push(Line::from(detail));
    }

    if let Some(error) = error_line {
        lines.push(Line::from(error));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(status_line));

    let popup = centered_rect(50, 40, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Operation")),
        popup,
    );
}

/// Shows the conflicts overlay (T-232/US-080; T-233/US-081): every
/// conflicted file [`gitsail_domain::InProgressOperation::conflicted_files`]
/// reports, the highlighted one's base/ours/theirs sides once inspected
/// (`Enter`), and the continue/abort actions
/// [`gitsail_domain::InProgressOperation::capabilities`] actually offers for
/// whatever operation is detected — never a fixed continue/abort pair
/// assumed for every kind (a bisect run, for instance, offers neither here).
fn render_conflicts_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let operation = app.in_progress_operation();
    let files = operation.conflicted_files();

    let mut lines = vec![Line::from(format!(
        "{} — {} conflicted file{}",
        operation.kind_label().unwrap_or("operation"),
        files.len(),
        if files.len() == 1 { "" } else { "s" }
    ))];
    lines.push(Line::from(""));

    if files.is_empty() {
        lines.push(Line::from("No conflicted files remain."));
    }
    for (index, file) in files.iter().enumerate() {
        let marker = if index == app.conflict_cursor() {
            '>'
        } else {
            ' '
        };
        lines.push(Line::from(format!(
            "{marker} {} ({})",
            sanitize::safe_line(&file.path.to_string_lossy()),
            conflict_stage_label(file.stage)
        )));
    }

    lines.push(Line::from(""));
    if let Some(error) = app.conflict_error() {
        lines.push(Line::from(sanitize::safe_line(error.message())));
        lines.push(Line::from(""));
    }

    match app.inspected_conflict() {
        Some(sides) => {
            lines.push(Line::from(format!(
                "base:   {}",
                conflict_side_label(&sides.base)
            )));
            lines.push(Line::from(format!(
                "ours:   {}",
                conflict_side_label(&sides.ours)
            )));
            lines.push(Line::from(format!(
                "theirs: {}",
                conflict_side_label(&sides.theirs)
            )));
        }
        None => lines.push(Line::from("Enter inspects the highlighted file's sides.")),
    }

    lines.push(Line::from(""));
    let mut actions = vec!["r resolves".to_string(), "o/t take ours/theirs".to_string()];
    if operation.supports(gitsail_domain::OperationCapability::Continue) {
        actions.push("c continues".to_string());
    }
    if operation.supports(gitsail_domain::OperationCapability::Abort) {
        actions.push("a aborts".to_string());
    }
    if operation.supports(gitsail_domain::OperationCapability::Skip) {
        actions.push("s skips".to_string());
    }
    actions.push("Esc/q closes".to_string());
    lines.push(Line::from(actions.join(" · ")));

    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Conflicts")),
        popup,
    );
}

fn conflict_stage_label(stage: ConflictStage) -> &'static str {
    match stage {
        ConflictStage::BothModified => "both modified",
        ConflictStage::BothAdded => "both added",
        ConflictStage::BothDeleted => "both deleted",
        ConflictStage::AddedByUs => "added by us",
        ConflictStage::AddedByThem => "added by them",
        ConflictStage::DeletedByUs => "deleted by us",
        ConflictStage::DeletedByThem => "deleted by them",
    }
}

fn conflict_side_label(content: &ConflictSideContent) -> String {
    match content {
        ConflictSideContent::Text(text) => {
            let first_line = text.lines().next().unwrap_or("");
            format!(
                "{} ({} line(s))",
                sanitize::safe_line(first_line),
                text.lines().count()
            )
        }
        ConflictSideContent::Binary => "<binary content>".to_string(),
        ConflictSideContent::Absent => "<absent>".to_string(),
    }
}

/// A short, fixed-width label for a [`RebaseAction`], used by
/// [`render_rebase_plan_overlay`] — never derived from repository content,
/// so it needs no [`sanitize`] pass.
fn rebase_action_label(action: RebaseAction) -> &'static str {
    match action {
        RebaseAction::Pick => "pick  ",
        RebaseAction::Reword => "reword",
        RebaseAction::Squash => "squash",
        RebaseAction::Fixup => "fixup ",
        RebaseAction::Drop => "drop  ",
    }
}

/// Shows the interactive rebase plan overlay (`O`, T-236/US-084 criterion
/// 1): the candidate commit range [`gitsail_application::PlanRebase`]
/// returned, in order, each entry's currently assigned action, and a
/// client-side validation error when one is pending (criterion 2) — never
/// the plan's *execution* result, which instead flows through
/// [`render_operation_overlay`] exactly like [`OperationKind::Merge`]/
/// [`OperationKind::Rebase`] once confirmed (see
/// [`crate::app::App::dispatch_operation`]'s own doc for why the overlay
/// itself closes at that point).
fn render_rebase_plan_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    match app.rebase_plan() {
        Some(plan) => {
            lines.push(Line::from(format!(
                "{} candidate commit(s) onto '{}'",
                plan.entries.len(),
                plan.onto_revision
            )));
            lines.push(Line::from(""));
            if plan.entries.is_empty() {
                lines.push(Line::from("Nothing to reapply — already up to date."));
            }
            for (index, entry) in plan.entries.iter().enumerate() {
                let marker = if index == app.rebase_plan_cursor() {
                    '>'
                } else {
                    ' '
                };
                let mut line = format!(
                    "{marker} {} {} {}",
                    rebase_action_label(entry.action),
                    entry.short_hash.as_str(),
                    sanitize::safe_line(&entry.subject)
                );
                if let Some(message) = &entry.message_override {
                    line.push_str(&format!(" -> {}", sanitize::safe_line(message)));
                }
                lines.push(Line::from(line));
            }
        }
        None => {
            if app.rebase_plan_error().is_none() {
                lines.push(Line::from("Loading the candidate commit range…"));
            }
        }
    }

    lines.push(Line::from(""));
    if let Some(error) = app.rebase_plan_error() {
        lines.push(Line::from(sanitize::safe_line(error.message())));
        if let Some(remediation) = error.remediation() {
            lines.push(Line::from(sanitize::safe_line(remediation)));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(
        "j/k select · J/K move entry · a cycle pick/reword/squash/fixup/drop · Enter confirms · Esc closes",
    ));

    let popup = centered_rect(75, 75, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Rebase Plan")),
        popup,
    );
}

/// Shows the Reword message prompt layered over the rebase plan overlay
/// (T-236/US-084's "reaproveite o mecanismo de input de texto"), mirroring
/// [`render_commit_composer`]'s own shape.
fn render_rebase_plan_reword(frame: &mut Frame, area: Rect, app: &App) {
    let subject = app
        .rebase_plan()
        .and_then(|plan| plan.entries.get(app.rebase_plan_cursor()))
        .map(|entry| entry.subject.as_str())
        .unwrap_or("");
    let lines = vec![
        Line::from(format!("Original: {}", sanitize::safe_line(subject))),
        Line::from(""),
        Line::from("New message:"),
        Line::from(sanitize::safe_line(
            app.rebase_plan_reword_input().unwrap_or(""),
        )),
        Line::from(""),
        Line::from("Enter confirms · Esc cancels"),
    ];
    let popup = centered_rect(70, 45, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Reword")),
        popup,
    );
}

/// Shows the reset-mode chooser (`z`, T-240/US-088 criterion 1): each of
/// Git's three modes with its own concrete, distinct effect on
/// `HEAD`/the index/the working tree spelled out in place — never a bare
/// "soft/mixed/hard" label a person has to already know the meaning of —
/// and, for `Hard`, the exact live count of uncommitted changes that would
/// be permanently discarded if it were chosen right now (US-088 criterion
/// 2), so the loss is visible *before* `Enter` even starts the reinforced
/// confirmation [`render_operation_overlay`] shows next.
fn render_reset_mode_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    if let Some(commit) = app.reset_target() {
        lines.push(Line::from(format!(
            "Reset to {} — {}",
            commit.short_hash.as_str(),
            sanitize::safe_line(&commit.subject)
        )));
        lines.push(Line::from(""));
    }

    let predicted_loss = app.predicted_reset_loss_file_count();
    let modes: [(ResetMode, &str); 3] = [
        (
            ResetMode::Soft,
            "Soft — HEAD moves only; index and working tree preserved (changes become staged)",
        ),
        (
            ResetMode::Mixed,
            "Mixed — HEAD and index move; working tree preserved (changes become unstaged)",
        ),
        (
            ResetMode::Hard,
            "Hard — HEAD, index and working tree all move",
        ),
    ];
    for (index, (mode, description)) in modes.iter().enumerate() {
        let marker = if index == app.reset_mode_cursor() {
            '>'
        } else {
            ' '
        };
        let mut line = format!("{marker} {description}");
        if *mode == ResetMode::Hard {
            line.push_str(&format!(
                " — {predicted_loss} uncommitted change{} would be PERMANENTLY DISCARDED",
                if predicted_loss == 1 { "" } else { "s" }
            ));
        }
        lines.push(Line::from(line));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("j/k select · Enter confirms · Esc closes"));

    let popup = centered_rect(75, 50, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Reset Mode")),
        popup,
    );
}

fn render_branch_name_prompt(frame: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::from("New branch name:"),
        Line::from(sanitize::safe_line(app.branch_input().unwrap_or(""))),
        Line::from(""),
        Line::from("Enter continues · Esc cancels"),
    ];
    let popup = centered_rect(50, 25, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(overlay_block(Theme::new(app.low_color()), "New Branch")),
        popup,
    );
}

/// Shows the commit composer (US-047 criterion 2: "composer aceita mensagem
/// e mostra escopo staged"). The footer reflects `app.operation()` so a
/// hook failure's message and remediation are visible without losing the
/// typed commit message or the staged scope above it (criterion 3).
fn render_commit_composer(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![Line::from("Staged changes:")];
    let staged: Vec<_> = app
        .status_entries()
        .into_iter()
        .filter(|entry| entry.scope == DiffScope::Staged)
        .collect();
    if staged.is_empty() {
        lines.push(Line::from("  (nothing staged)"));
    } else {
        for entry in &staged {
            lines.push(Line::from(format!(
                "  {}",
                sanitize::safe_path(&entry.path)
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Message:"));
    lines.push(Line::from(sanitize::safe_line(
        app.commit_message().unwrap_or(""),
    )));
    lines.push(Line::from(""));

    let footer = match app.operation() {
        OperationState::Idle => "Enter reviews · Esc discards".to_string(),
        OperationState::Confirming(_) => "Enter commits · Esc cancels (message kept)".to_string(),
        OperationState::InProgress(_) => "Committing…".to_string(),
        OperationState::Succeeded(_) => "Done".to_string(),
        OperationState::Failed(_, err) => format!(
            "{} — Esc dismisses (message kept)",
            sanitize::safe_line(err.message())
        ),
    };
    lines.push(Line::from(footer));

    let popup = centered_rect(70, 60, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(Theme::new(app.low_color()), "Commit")),
        popup,
    );
}

/// Shows the amend composer (T-242/US-090), mirroring
/// [`render_commit_composer`]'s own shape exactly: `HEAD`'s identity and the
/// staged-file count that would be folded in (from the already-loaded
/// [`gitsail_application::AmendPreview`] — US-090 criterion 1's read-only
/// preview), the editable message, and a footer that reflects
/// `app.operation()`. Unlike the commit composer, `Confirming` shows the
/// exact same Destructive risk text/target label
/// [`crate::operation::OperationKind::AmendCommit::prompt_label`] renders
/// everywhere else (US-090 criterion 2: the commit being replaced and the
/// publication risk are both named explicitly, never a generic "are you
/// sure?").
fn render_amend_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    match app.amend_preview() {
        Some(preview) => {
            lines.push(Line::from(format!(
                "HEAD: {} — {}",
                preview.head.short_hash.as_str(),
                sanitize::safe_line(&preview.head.subject)
            )));
            lines.push(Line::from(format!(
                "{} staged file{} will be folded into the amended commit.",
                preview.staged_diff.files.len(),
                if preview.staged_diff.files.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )));
            lines.push(Line::from(""));
            lines.push(Line::from("Message:"));
            lines.push(Line::from(sanitize::safe_line(
                app.amend_message().unwrap_or(""),
            )));
            lines.push(Line::from(""));
        }
        None => {
            if let Some(error) = app.amend_error() {
                lines.push(Line::from(sanitize::safe_line(error.message())));
                lines.push(Line::from(""));
            } else {
                lines.push(Line::from(
                    "Loading HEAD's current commit and staged changes…",
                ));
                lines.push(Line::from(""));
            }
        }
    }

    let footer = match app.operation() {
        OperationState::Idle => "Enter reviews · Esc discards".to_string(),
        OperationState::Confirming(kind) => format!(
            "{}\nrisk: {:?}\nEnter amends · Esc cancels (message kept)",
            sanitize::safe_line(&kind.prompt_label()),
            kind.risk()
        ),
        OperationState::InProgress(_) => "Amending…".to_string(),
        OperationState::Succeeded(_) => "Done".to_string(),
        OperationState::Failed(_, err) => format!(
            "{} — Esc dismisses (message kept)",
            sanitize::safe_line(err.message())
        ),
    };
    for line in footer.split('\n') {
        lines.push(Line::from(line.to_string()));
    }

    let popup = centered_rect(75, 65, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(
                Theme::new(app.low_color()),
                "Amend Last Commit",
            )),
        popup,
    );
}

fn render_help(frame: &mut Frame, area: Rect, theme: Theme) {
    let popup = centered_rect(60, 75, area);
    let lines = vec![
        Line::from("Keyboard shortcuts"),
        Line::from(""),
        Line::from("Tab / Shift+Tab   move focus between panels"),
        Line::from("Up/Down, j/k      move the selection"),
        Line::from("Enter             select / open / confirm"),
        Line::from("Enter (Graph)     open commit details (hash, author, message)"),
        Line::from("/ (Sidebar)       filter the branch list"),
        Line::from("/ (Graph)         search commits: text, author:, branch:, hash"),
        Line::from("b                 toggle diff/blame view"),
        Line::from("y (Diff)          copy the diff's patch (or save to a file)"),
        Line::from("Y (Diff)          apply the patch on the clipboard (preview, then confirm)"),
        Line::from("s                 stage/unstage the highlighted entry"),
        Line::from("C                 compose a commit"),
        Line::from("n                 create a branch"),
        Line::from("c                 checkout the highlighted branch"),
        Line::from("d                 delete the highlighted branch"),
        Line::from("f                 fetch the resolved remote"),
        Line::from("p                 pull (fast-forward only)"),
        Line::from("P                 push the current branch"),
        Line::from("o (Sidebar)       rebase the current branch onto the highlighted branch"),
        Line::from("O (Sidebar)       plan an interactive rebase onto the highlighted branch"),
        Line::from("  within the plan: j/k select · J/K reorder · a cycle action · Enter confirms"),
        Line::from("x (Graph)         cherry-pick the highlighted commit onto the current branch"),
        Line::from("v (Graph)         revert the highlighted commit"),
        Line::from("z (Graph)         reset to the highlighted commit (choose soft/mixed/hard)"),
        Line::from("t (References)    cycle Tags/Remotes/Stash/Reflog"),
        Line::from("Enter (References) view the highlighted entry's details"),
        Line::from("  (Reflog) opens the entry's commit details, when it still exists"),
        Line::from("A                 amend HEAD (compose the new message, then confirm)"),
        Line::from("w                 open in browser (branch/commit/repo, when a GitHub/GitLab remote is detected)"),
        Line::from("Esc               cancel a pending confirmation · dismiss a result"),
        Line::from("  a confirmation is modal: while it is up, Enter and Esc are the only keys"),
        Line::from("r                 refresh status and branches"),
        Line::from("?                 toggle this help"),
        Line::from("q, Ctrl+C         quit"),
        Line::from(""),
        Line::from("Press ? or Esc to close"),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(overlay_block(theme, "Help")),
        popup,
    );
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let text = format!(
        "Terminal too small ({}x{}). Resize to at least {MIN_WIDTH}x{MIN_HEIGHT}.",
        area.width, area.height
    );
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod format_timestamp_tests {
    use super::*;

    #[test]
    fn renders_a_calendar_date_in_the_commit_s_own_offset() {
        // 2024-01-15T12:30:00+02:00 (a fixed, hand-verified Unix instant).
        let ts = GitTimestamp::new(1_705_314_600, 120);
        assert_eq!(format_timestamp(&ts), "2024-01-15 12:30 (+02:00)");
    }

    #[test]
    fn a_negative_offset_shifts_the_calendar_date_backward_across_midnight() {
        // 2024-01-01T00:30:00Z shown at -02:00 falls back to 2023-12-31.
        let ts = GitTimestamp::new(1_704_069_000, -120);
        assert_eq!(format_timestamp(&ts), "2023-12-31 22:30 (-02:00)");
    }

    #[test]
    fn the_unix_epoch_itself_renders_as_1970_01_01() {
        let ts = GitTimestamp::new(0, 0);
        assert_eq!(format_timestamp(&ts), "1970-01-01 00:00 (+00:00)");
    }
}
