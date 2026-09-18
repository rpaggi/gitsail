//! The "Render" stage of SAD §18's event/update/render model (US-040,
//! US-043, US-046, US-047, US-048). Reads [`App`] but never mutates it.
//!
//! Every piece of text that originates from the repository (file/branch
//! names, diff/blame content) is passed through [`crate::sanitize`] before
//! reaching a widget (SAD §33) — this module is the render boundary that
//! rule applies at; `App` itself always holds the raw value.

use crate::graph_view;
use gitsail_application::{
    CherryPickResult, MergeResult, PullOutcome, RebaseAction, RebaseResult, ResetMode, RevertResult,
};
use gitsail_domain::{
    BlameOrigin, BranchKind, Commit, ConflictSideContent, ConflictStage, DiffLineOrigin,
    GitTimestamp, TagKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{
    App, DiffViewMode, Panel, PatchApplyOutcome, PatchExportOutcome, ReferenceView, ViewPhase,
};
use crate::operation::OperationState;
use crate::sanitize;
use crate::status_view::DiffScope;

/// Below this width or height the five-panel layout has no room left to be
/// legible; a single message replaces it instead (US-043 criterion 2:
/// "Resize adapta painéis e comunica largura mínima quando necessário").
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 16;

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(frame, area);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(0)])
        .split(outer[0]);

    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(main[1]);

    // Three columns rather than the original two (US-050 adds the
    // References panel alongside Details/Diff): even thirds keep each one
    // legible without shrinking Details/Diff dramatically.
    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(content[1]);

    render_sidebar(frame, main[0], app);
    render_graph_panel(frame, content[0], app);
    render_status_panel(frame, lower[0], app);
    render_diff_panel(frame, lower[1], app);
    render_references_panel(frame, lower[2], app);
    render_shortcuts(frame, outer[1], app);

    render_overlays(frame, area, app);
}

/// Focus is always conveyed through the title text (a `»` marker), never
/// through color alone — color is layered on top only when `low_color` is
/// false, so the terminal-capability-reduced path (US-043 criterion 3)
/// still distinguishes every state without depending on it.
fn panel_block(title: &str, focused: bool, low_color: bool) -> Block<'static> {
    let display_title = if focused {
        format!("» {title}")
    } else {
        title.to_string()
    };
    let mut style = Style::default();
    if focused {
        style = style.add_modifier(Modifier::BOLD);
        if !low_color {
            style = style.fg(Color::Cyan);
        }
    }
    Block::default()
        .title(display_title)
        .borders(Borders::ALL)
        .border_style(style)
}

/// The placeholder text for a phase that has not reached `Loaded` yet.
fn phase_placeholder_text(phase: ViewPhase, panel: Panel) -> String {
    match phase {
        ViewPhase::Loading => "Loading…".to_string(),
        ViewPhase::Empty => "Nothing to show — empty repository.".to_string(),
        ViewPhase::Error => "Unavailable — see the sidebar for the error.".to_string(),
        ViewPhase::Loaded => format!("{} view not implemented yet.", panel.title()),
    }
}

fn branch_summary_line(app: &App) -> String {
    match app.view_phase() {
        ViewPhase::Loading => "branch: loading…".to_string(),
        ViewPhase::Error => "branch: unavailable (error)".to_string(),
        ViewPhase::Empty | ViewPhase::Loaded => app
            .session()
            .and_then(|s| s.repository().current_branch.as_ref())
            .map(|b| format!("branch: {}", b.as_str()))
            .unwrap_or_else(|| "branch: (detached HEAD)".to_string()),
    }
}

fn status_summary_line(app: &App) -> String {
    match app.view_phase() {
        ViewPhase::Loading => "status: loading…".to_string(),
        ViewPhase::Empty => "status: empty repository (no commits yet)".to_string(),
        ViewPhase::Error => {
            let message = app
                .status_error()
                .or(app.discovery_error())
                .map(|e| e.message())
                .unwrap_or("unknown error");
            format!("status: error — {message}")
        }
        ViewPhase::Loaded => {
            let changed = app
                .session()
                .and_then(|s| s.status())
                .map(|s| s.files.len())
                .unwrap_or(0);
            if changed == 0 {
                "status: clean".to_string()
            } else {
                format!("status: {changed} changed file(s)")
            }
        }
    }
}

fn render_sidebar(frame: &mut Frame, rect: Rect, app: &App) {
    let block = panel_block(
        Panel::Sidebar.title(),
        app.focus() == Panel::Sidebar,
        app.low_color(),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut header = vec![
        Line::from(format!("repo: {}", app.repo_path().display())),
        Line::from(branch_summary_line(app)),
        Line::from(status_summary_line(app)),
    ];
    if let Some(query) = app.search() {
        header.push(Line::from(format!("/{query}")));
    }
    let header_height = header.len() as u16;

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(Paragraph::new(header), split[0]);

    // US-048 criterion 1: local/remote and the current branch must all be
    // distinguishable from this list alone.
    let items: Vec<ListItem> = app
        .filtered_branches()
        .iter()
        .enumerate()
        .map(|(i, branch)| {
            let marker = if branch.is_current { "*" } else { " " };
            let name = sanitize::safe_line(branch.name.as_str());
            let text = match &branch.kind {
                BranchKind::Local => format!("{marker} {name}"),
                BranchKind::Remote { remote } => {
                    format!("{marker} {name}  [{}]", sanitize::safe_line(remote))
                }
            };
            let style = if i == app.sidebar_cursor() {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();
    frame.render_widget(List::new(items), split[1]);
}

/// Renders the Graph panel: the shared commit-graph layout from
/// [`gitsail_domain::graph`], turned into text by [`crate::graph_view`]
/// (US-066 criterion 1). Uses Ratatui's own [`ListState`] scrolling rather
/// than hand-rolled offset math, so the highlighted row is always kept in
/// view as `graph_cursor` moves or the terminal resizes (US-066 criterion
/// 3).
fn render_graph_panel(frame: &mut Frame, rect: Rect, app: &App) {
    // The title carries the *submitted* filter (US-045 criterion 2) — the
    // box being edited right now is shown as its own header line below,
    // exactly like the Sidebar shows `app.search()` (US-042), so the two
    // never compete for the same line.
    let title = match app.active_commit_filter() {
        Some(filter) => format!(
            "{} (filter: {})",
            Panel::Graph.title(),
            sanitize::safe_line(filter)
        ),
        None => Panel::Graph.title().to_string(),
    };
    let block = panel_block(&title, app.focus() == Panel::Graph, app.low_color());

    if app.view_phase() != ViewPhase::Loaded {
        let text = phase_placeholder_text(app.view_phase(), Panel::Graph);
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
            rect,
        );
        return;
    }

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // While the commit-search box is open, its own line is reserved above
    // the list — like the Sidebar reserves one for `app.search()` — so
    // typing is visible without covering the currently loaded rows.
    let content_area = if let Some(query) = app.commit_search() {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(Paragraph::new(format!("/{query}")), split[0]);
        split[1]
    } else {
        inner
    };

    if let Some(error) = app.graph_error() {
        frame.render_widget(
            Paragraph::new(sanitize::safe_line(error.message())).wrap(Wrap { trim: true }),
            content_area,
        );
        return;
    }

    let rows = app.commit_graph().rows();
    if rows.is_empty() {
        let text = if app.graph_loading() {
            "Loading history…"
        } else if app.active_commit_filter().is_some() {
            "No commits match this search."
        } else {
            "No commits yet."
        };
        frame.render_widget(Paragraph::new(text), content_area);
        return;
    }

    let lines = graph_view::render_rows(rows, app.graph_commits(), app.commit_graph().lane_count());
    let mut items: Vec<ListItem> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let style = if app.focus() == Panel::Graph && i == app.graph_cursor() {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(line.text.clone()).style(style)
        })
        .collect();
    if app.graph_loading() {
        items.push(ListItem::new("Loading more…"));
    }

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.graph_cursor()));
    frame.render_stateful_widget(List::new(items), content_area, &mut state);
}

/// Renders the Details panel as the status list (US-046 criterion 1):
/// index and worktree changes are distinct, independently selectable rows.
fn render_status_panel(frame: &mut Frame, rect: Rect, app: &App) {
    let block = panel_block(
        Panel::Details.title(),
        app.focus() == Panel::Details,
        app.low_color(),
    );

    if app.view_phase() != ViewPhase::Loaded {
        let text = phase_placeholder_text(app.view_phase(), Panel::Details);
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
            rect,
        );
        return;
    }

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let entries = app.status_entries();
    if entries.is_empty() {
        frame.render_widget(Paragraph::new("No changes."), inner);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let scope_tag = match entry.scope {
                DiffScope::Staged => "S",
                DiffScope::Worktree => "W",
            };
            let text = format!(
                "{scope_tag} {:?} {}",
                entry.change_type,
                sanitize::safe_path(&entry.path)
            );
            let style = if app.focus() == Panel::Details && i == app.status_cursor() {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// Renders the Diff panel: either the diff or the blame of the currently
/// selected file, depending on [`DiffViewMode`] (US-046 criteria 2, 3).
fn render_diff_panel(frame: &mut Frame, rect: Rect, app: &App) {
    let title = match app.diff_view_mode() {
        DiffViewMode::Diff => Panel::Diff.title().to_string(),
        DiffViewMode::Blame => format!("{} — Blame", Panel::Diff.title()),
    };
    let block = panel_block(&title, app.focus() == Panel::Diff, app.low_color());

    if app.view_phase() != ViewPhase::Loaded {
        let text = phase_placeholder_text(app.view_phase(), Panel::Diff);
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
            rect,
        );
        return;
    }

    let lines: Vec<Line<'static>> = if app.selected_file().is_none() {
        vec![Line::from(
            "No file selected — press Enter on a status entry.",
        )]
    } else {
        match app.diff_view_mode() {
            DiffViewMode::Diff => diff_lines(app),
            DiffViewMode::Blame => blame_lines(app),
        }
    };

    let scroll = match app.diff_view_mode() {
        DiffViewMode::Blame => app.blame_scroll(),
        DiffViewMode::Diff => 0,
    };

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(block),
        rect,
    );
}

/// Content lines for the diff sub-view: binary/truncated banners (US-046
/// criterion 2) come before any hunk, never in place of one — a truncated
/// file's withheld hunks are never confused with "no changes".
fn diff_lines(app: &App) -> Vec<Line<'static>> {
    // A patch-export result (US-029 criterion 1) is shown regardless of
    // which of the states below applies — even "Loading diff…" or an error
    // — since it reports on the *previous* action, not on what is
    // currently loading; an `if let ... return` chain would otherwise hide
    // it whenever one of those early states applies.
    let banner = app.patch_export().map(patch_export_banner);
    let apply_banner = app.patch_apply_outcome().map(patch_apply_banner);

    let mut lines = Vec::new();
    if let Some(err) = app.diff_error() {
        lines.push(Line::from(sanitize::safe_line(err.message())));
    } else if let Some(diff) = app.diff() {
        if let Some(file) = diff.files.first() {
            if file.is_binary {
                lines.push(Line::from("[binary file]"));
            }
            if file.truncated {
                lines.push(Line::from("[diff truncated — content withheld, not empty]"));
            }
            for (i, hunk) in file.hunks.iter().enumerate() {
                let header = format!(
                    "@@ -{},{} +{},{} @@",
                    hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                );
                let header_style = if i == app.diff_hunk_cursor() {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };
                lines.push(Line::from(header).style(header_style));
                for line in &hunk.lines {
                    let prefix = match line.origin {
                        DiffLineOrigin::Addition => '+',
                        DiffLineOrigin::Deletion => '-',
                        DiffLineOrigin::Context => ' ',
                    };
                    lines.push(Line::from(format!(
                        "{prefix}{}",
                        sanitize::safe_line(&line.content)
                    )));
                }
            }
            if lines.is_empty() {
                lines.push(Line::from("No hunks."));
            }
        } else {
            lines.push(Line::from("No changes for this file."));
        }
    } else {
        lines.push(Line::from("Loading diff…"));
    }

    if let Some(banner) = apply_banner {
        lines.insert(0, banner);
    }
    if let Some(banner) = banner {
        lines.insert(0, banner);
    }
    lines
}

/// Renders the outcome of the last `y` (export patch) press as a single,
/// bold banner line (US-029 criterion 1: origin/scope, criterion 3:
/// clipboard vs. file-fallback outcome — both stated explicitly, never left
/// for the person to infer).
fn patch_export_banner(outcome: &PatchExportOutcome) -> Line<'static> {
    let text = match outcome {
        PatchExportOutcome::Copied {
            scope,
            file_count,
            incomplete,
        } => format!(
            "Patch copied to clipboard — {scope} ({file_count} file{}){}",
            if *file_count == 1 { "" } else { "s" },
            if *incomplete {
                " [incomplete: binary/truncated content skipped]"
            } else {
                ""
            }
        ),
        PatchExportOutcome::SavedToFile {
            scope,
            path,
            incomplete,
            reason,
        } => format!(
            "Clipboard unavailable ({reason}) — patch for {scope} saved to {}{}",
            path.display(),
            if *incomplete {
                " [incomplete: binary/truncated content skipped]"
            } else {
                ""
            }
        ),
        PatchExportOutcome::Failed { reason } => format!("Patch export failed: {reason}"),
        PatchExportOutcome::Empty => {
            "Nothing to export — no content hunks in the current diff".to_string()
        }
    };
    Line::from(sanitize::safe_line(&text)).style(Style::default().add_modifier(Modifier::BOLD))
}

/// Renders the outcome of the last `Y` (apply patch) press as a single,
/// bold banner line (T-163/US-030 criteria 1-3), mirroring
/// [`patch_export_banner`]'s own convention.
fn patch_apply_banner(outcome: &PatchApplyOutcome) -> Line<'static> {
    let text = match outcome {
        PatchApplyOutcome::ClipboardEmpty { reason } => {
            format!("Nothing to apply from the clipboard ({reason})")
        }
        PatchApplyOutcome::Rejected { reason } => format!("Patch rejected: {reason}"),
        PatchApplyOutcome::Failed { reason } => format!("Patch apply failed: {reason}"),
        PatchApplyOutcome::Applied { affected_files } => format!(
            "Patch applied — {} file{} changed",
            affected_files.len(),
            if affected_files.len() == 1 { "" } else { "s" }
        ),
    };
    Line::from(sanitize::safe_line(&text)).style(Style::default().add_modifier(Modifier::BOLD))
}

/// Content lines for the blame sub-view (US-046 criterion 3): line number,
/// author, and commit — `BlameOrigin::Local` is shown as `"local"` rather
/// than a synthetic commit, so uncommitted content is never mistaken for a
/// real attribution (US-033).
fn blame_lines(app: &App) -> Vec<Line<'static>> {
    if let Some(err) = app.blame_error() {
        return vec![Line::from(sanitize::safe_line(err.message()))];
    }
    let Some(blame) = app.blame() else {
        return vec![Line::from("Loading blame…")];
    };
    if blame.lines.is_empty() {
        return vec![Line::from("No lines to blame.")];
    }
    blame
        .lines
        .iter()
        .map(|line| {
            let attribution = match line.origin {
                BlameOrigin::Local => "local".to_string(),
                BlameOrigin::Committed => line.commit.to_short(8).as_str().to_string(),
            };
            Line::from(format!(
                "{attribution:<8} {:<20} {:>5}  {}",
                sanitize::safe_line(&line.author.name),
                line.final_line,
                sanitize::safe_line(&line.content)
            ))
        })
        .collect()
}

/// Renders the References panel: whichever of Tags/Remotes/Stash
/// [`ReferenceView`] currently selects (US-050 criterion 1), with an
/// explicit empty state per sub-view (criterion 3) rather than a single
/// generic "nothing here" for all three.
fn render_references_panel(frame: &mut Frame, rect: Rect, app: &App) {
    let title = format!(
        "{} — {}",
        Panel::References.title(),
        app.reference_view().title()
    );
    let block = panel_block(&title, app.focus() == Panel::References, app.low_color());

    if app.view_phase() != ViewPhase::Loaded {
        let text = phase_placeholder_text(app.view_phase(), Panel::References);
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
            rect,
        );
        return;
    }

    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines: Vec<String> = match app.reference_view() {
        ReferenceView::Tags => {
            if app.tags().is_empty() {
                vec!["No tags.".to_string()]
            } else {
                app.tags()
                    .iter()
                    .map(|tag| {
                        let kind = match &tag.kind {
                            TagKind::Lightweight => "lightweight".to_string(),
                            TagKind::Annotated { .. } => "annotated".to_string(),
                        };
                        format!(
                            "{} -> {} [{kind}]",
                            sanitize::safe_line(&tag.name),
                            tag.target.to_short(8).as_str()
                        )
                    })
                    .collect()
            }
        }
        ReferenceView::Remotes => {
            if app.remotes().is_empty() {
                vec!["No remotes configured.".to_string()]
            } else {
                app.remotes()
                    .iter()
                    .map(|remote| {
                        // `Remote`'s URLs render already-redacted via their
                        // own `Display` (SAD §11, §28) — never the raw
                        // credential-bearing string.
                        format!(
                            "{}  fetch={} push={}",
                            sanitize::safe_line(&remote.name),
                            sanitize::safe_line(&remote.fetch_url.to_string()),
                            sanitize::safe_line(&remote.push_url.to_string())
                        )
                    })
                    .collect()
            }
        }
        ReferenceView::Stash => {
            if app.stashes().is_empty() {
                vec!["No stash entries.".to_string()]
            } else {
                app.stashes()
                    .iter()
                    .map(|stash| {
                        format!(
                            "stash@{{{}}} {} {}",
                            stash.index,
                            stash.commit.to_short(8).as_str(),
                            sanitize::safe_line(&stash.message)
                        )
                    })
                    .collect()
            }
        }
        // T-241/US-089 criterion 1: reference (`HEAD@{n}`), hash, and
        // message are all shown; criterion 3: an entry whose object no
        // longer exists is marked `[missing]` right in the list, never
        // silently hidden.
        ReferenceView::Reflog => {
            if app.reflog().is_empty() {
                vec!["No reflog entries.".to_string()]
            } else {
                app.reflog()
                    .iter()
                    .map(|entry| {
                        let missing = if entry.is_available() {
                            ""
                        } else {
                            " [missing]"
                        };
                        format!(
                            "{} {} {}{missing}",
                            entry.selector("HEAD"),
                            entry.commit.to_short(8).as_str(),
                            sanitize::safe_line(&entry.message)
                        )
                    })
                    .collect()
            }
        }
    };

    let has_entries = app.reference_len() > 0;
    let items: Vec<ListItem> = lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let style =
                if has_entries && app.focus() == Panel::References && i == app.reference_cursor() {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
            ListItem::new(text).style(style)
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

fn render_shortcuts(frame: &mut Frame, rect: Rect, app: &App) {
    let text = if app.commit_details_open() {
        "Esc/q closes commit details".to_string()
    } else if app.reference_details_open() {
        "Esc/q closes reference details".to_string()
    } else if app.reflog_details_open() {
        "Esc/q closes reflog entry details".to_string()
    } else if app.amend_open() {
        "Type amend message · Enter confirms · Esc discards".to_string()
    } else if app.conflicts_open() {
        "j/k select · Enter inspects · r resolves · o/t take ours/theirs · c continue · a abort · s skip · Esc closes"
            .to_string()
    } else if app.rebase_plan_reword_input().is_some() {
        "Type the new message · Enter confirms · Esc cancels".to_string()
    } else if app.rebase_plan_open() {
        "j/k select · J/K move entry · a cycle action · Enter confirms plan · Esc closes"
            .to_string()
    } else if app.in_progress_operation().has_conflicts() {
        format!(
            "{} conflicted file(s) — press 'M' to resolve them",
            app.in_progress_operation().conflicted_files().len()
        )
    } else if app.commit_search().is_some() {
        "Type message/author:/branch:/hash · Enter searches · Esc cancels".to_string()
    } else if app.search().is_some() {
        "Type to filter · Enter/Esc close search".to_string()
    } else if app.branch_input().is_some() {
        "Type branch name · Enter confirms · Esc cancels".to_string()
    } else if app.commit_message().is_some() {
        "Type commit message · Enter confirms · Esc discards".to_string()
    } else {
        // Kept short enough to fit a narrow terminal (down to `MIN_WIDTH`);
        // the full shortcut list — including the mutating keys added by
        // US-046/047/048 — lives in the `?` help overlay instead.
        "Tab focus · j/k move · Enter act · / search · r refresh · ? help · q quit".to_string()
    };
    frame.render_widget(Paragraph::new(text), rect);
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
        render_help(frame, area);
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
            .block(
                Block::default()
                    .title("Commit Details")
                    .borders(Borders::ALL),
            ),
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
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Reference Details")
                .borders(Borders::ALL),
        ),
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
            .block(Block::default().title("Reflog Entry").borders(Borders::ALL)),
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
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Open in Browser")
                .borders(Borders::ALL),
        ),
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
            .block(Block::default().title("Remote Sync").borders(Borders::ALL)),
        popup,
    );
}

/// Shows a pending/running/finished mutation (US-047, US-048): what it
/// targets, its SAD §20 risk tier, and — for `SwitchBranch`/`DeleteBranch`
/// — the branch's current target as the "ref de origem" criterion 2 asks
/// for. A failure never implies anything was discarded (criterion 3).
fn render_operation_overlay(frame: &mut Frame, area: Rect, app: &App) {
    use crate::operation::OperationKind;

    let (kind, status_line, error_line) = match app.operation() {
        OperationState::Confirming(kind) => (kind, "Enter confirms · Esc cancels", None),
        OperationState::InProgress(kind) => (kind, "Working…", None),
        OperationState::Succeeded(kind) => (kind, "Done — press any key to dismiss", None),
        OperationState::Failed(kind, err) => (
            kind,
            "Nothing was changed — Esc dismisses",
            Some(sanitize::safe_line(err.message())),
        ),
        OperationState::Idle => return,
    };

    let mut lines = vec![
        Line::from(sanitize::safe_line(&kind.target_label())),
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
            lines.push(Line::from(text));
        }
    }

    // T-231/US-079 criterion 2/3: fast-forward, a new merge commit, and a
    // conflict are always three distinct, explicit lines — a conflict is
    // never left to be inferred from a bare "Done" (`status_line` above
    // already says so unconditionally, but this line is what actually
    // tells the two apart at a glance) and never silently treated the same
    // as either of the other two outcomes.
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
                    "CONFLICT — {} file{} need resolution (press 'M' once dismissed)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            lines.push(Line::from(text));
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
                    "CONFLICT — {} file{} need resolution (press 'M' once dismissed)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            lines.push(Line::from(text));
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
                    "CONFLICT — {} file{} need resolution (press 'M' once dismissed)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
                CherryPickResult::Empty => "EMPTY — already applied on the current branch (press 'M' to skip/abort once dismissed)".to_string(),
            };
            lines.push(Line::from(text));
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
                    "CONFLICT — {} file{} need resolution (press 'M' once dismissed)",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
            };
            lines.push(Line::from(text));
        }
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
            .block(Block::default().title("Operation").borders(Borders::ALL)),
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
            .block(Block::default().title("Conflicts").borders(Borders::ALL)),
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
            .block(Block::default().title("Rebase Plan").borders(Borders::ALL)),
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
            .block(Block::default().title("Reword").borders(Borders::ALL)),
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
            .block(Block::default().title("Reset Mode").borders(Borders::ALL)),
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
        Paragraph::new(lines).block(Block::default().title("New Branch").borders(Borders::ALL)),
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
            .block(Block::default().title("Commit").borders(Borders::ALL)),
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
/// [`crate::operation::OperationKind::AmendCommit::target_label`] renders
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
            sanitize::safe_line(&kind.target_label()),
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
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Amend Last Commit")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
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
        Line::from("r                 refresh status and branches"),
        Line::from("?                 toggle this help"),
        Line::from("q, Ctrl+C         quit"),
        Line::from(""),
        Line::from("Press ? or Esc to close"),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title("Help").borders(Borders::ALL)),
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
