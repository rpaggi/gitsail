//! The "Render" stage of SAD §18's event/update/render model (US-040,
//! US-043, US-046, US-047, US-048). Reads [`App`] but never mutates it.
//!
//! Every piece of text that originates from the repository (file/branch
//! names, diff/blame content) is passed through [`crate::sanitize`] before
//! reaching a widget (SAD §33) — this module is the render boundary that
//! rule applies at; `App` itself always holds the raw value.

use gitsail_domain::{BlameOrigin, BranchKind, DiffLineOrigin};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, DiffViewMode, Panel, ViewPhase};
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

    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content[1]);

    render_sidebar(frame, main[0], app);
    render_placeholder_panel(frame, content[0], Panel::Graph, app);
    render_status_panel(frame, lower[0], app);
    render_diff_panel(frame, lower[1], app);
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

/// The placeholder text for a phase that has not reached `Loaded` yet, or
/// (for a panel that never got its own `Loaded` content — only `Graph`
/// today) for `Loaded` itself.
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

fn render_placeholder_panel(frame: &mut Frame, rect: Rect, panel: Panel, app: &App) {
    let block = panel_block(panel.title(), app.focus() == panel, app.low_color());
    let text = phase_placeholder_text(app.view_phase(), panel);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
        rect,
    );
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
    if let Some(err) = app.diff_error() {
        return vec![Line::from(sanitize::safe_line(err.message()))];
    }
    let Some(diff) = app.diff() else {
        return vec![Line::from("Loading diff…")];
    };
    let Some(file) = diff.files.first() else {
        return vec![Line::from("No changes for this file.")];
    };

    let mut lines = Vec::new();
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
    lines
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

fn render_shortcuts(frame: &mut Frame, rect: Rect, app: &App) {
    let text = if app.search().is_some() {
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
    if app.commit_message().is_some() {
        render_commit_composer(frame, area, app);
    } else if !app.operation().is_idle() {
        render_operation_overlay(frame, area, app);
    } else if app.branch_input().is_some() {
        render_branch_name_prompt(frame, area, app);
    }

    if app.help_visible() {
        render_help(frame, area);
    }
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

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 75, area);
    let lines = vec![
        Line::from("Keyboard shortcuts"),
        Line::from(""),
        Line::from("Tab / Shift+Tab   move focus between panels"),
        Line::from("Up/Down, j/k      move the selection"),
        Line::from("Enter             select / open / confirm"),
        Line::from("/                 filter the branch list"),
        Line::from("b                 toggle diff/blame view"),
        Line::from("s                 stage/unstage the highlighted entry"),
        Line::from("C                 compose a commit"),
        Line::from("n                 create a branch"),
        Line::from("c                 checkout the highlighted branch"),
        Line::from("d                 delete the highlighted branch"),
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
