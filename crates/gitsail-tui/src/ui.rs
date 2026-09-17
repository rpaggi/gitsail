//! The "Render" stage of SAD §18's event/update/render model (US-040,
//! US-043). Reads [`App`] but never mutates it.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Panel, ViewPhase};

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
    render_placeholder_panel(frame, lower[0], Panel::Details, app);
    render_placeholder_panel(frame, lower[1], Panel::Diff, app);
    render_shortcuts(frame, outer[1], app);

    if app.help_visible() {
        render_help(frame, area);
    }
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

    let items: Vec<ListItem> = app
        .filtered_branches()
        .iter()
        .enumerate()
        .map(|(i, branch)| {
            let marker = if branch.is_current { "*" } else { " " };
            let text = format!("{marker} {}", branch.name.as_str());
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
    let text = match app.view_phase() {
        ViewPhase::Loading => "Loading…".to_string(),
        ViewPhase::Empty => "Nothing to show — empty repository.".to_string(),
        ViewPhase::Error => "Unavailable — see the sidebar for the error.".to_string(),
        ViewPhase::Loaded => format!("{} view not implemented yet.", panel.title()),
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
        rect,
    );
}

fn render_shortcuts(frame: &mut Frame, rect: Rect, app: &App) {
    let text = if app.search().is_some() {
        "Type to filter · Enter/Esc close search".to_string()
    } else {
        "Tab focus · j/k move · Enter select · / search · r refresh · ? help · q quit".to_string()
    };
    frame.render_widget(Paragraph::new(text), rect);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 60, area);
    let lines = vec![
        Line::from("Keyboard shortcuts"),
        Line::from(""),
        Line::from("Tab / Shift+Tab   move focus between panels"),
        Line::from("Up/Down, j/k      move the selection"),
        Line::from("Enter             select the highlighted branch"),
        Line::from("/                 filter the branch list"),
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
