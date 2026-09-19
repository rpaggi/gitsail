//! Palette, glyph set and row-layout primitives for [`crate::ui`].
//!
//! Split out of `ui.rs` for the same reason [`crate::graph_view`] and
//! [`crate::status_view`] are: everything here is pure and assertable with
//! plain string/style comparisons, with no [`ratatui::Frame`] in sight.
//!
//! The one invariant every helper here exists to enforce is US-043
//! criterion 3 (and the `NO_COLOR` convention `main.rs` folds into the same
//! flag): **no state may be distinguishable by color alone, and none by a
//! glyph a plain terminal font cannot draw**. So every accessor is a method
//! on [`Theme`], which knows whether `low_color` is set, and each one has a
//! degraded form — a color becomes "no color at all" plus a shape/modifier
//! that already carried the same meaning, and a decorative glyph becomes
//! its ASCII equivalent. Glyphs are picked from Latin-1 Supplement,
//! Arrows, Box Drawing, Block Elements, Geometric Shapes and Mathematical
//! Operators only; no Nerd Font/private-use codepoint is used anywhere.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::BorderType;
use unicode_width::UnicodeWidthStr;

/// The blue used for the active navigation row, the selected list row and
/// focused borders — the mockup's single accent hue.
const ACCENT: Color = Color::Indexed(33);
const ACCENT_SOFT: Color = Color::Indexed(75);
/// Short commit hashes, as in the mockup's cyan hash column.
const HASH: Color = Color::Indexed(80);
const MUTED: Color = Color::Indexed(245);
const BORDER: Color = Color::Indexed(238);
const OK: Color = Color::Indexed(77);
const WARN: Color = Color::Indexed(179);
const DANGER: Color = Color::Indexed(203);
const ADD_FG: Color = Color::Indexed(114);
const DEL_FG: Color = Color::Indexed(210);
const ADD_BG: Color = Color::Indexed(22);
const DEL_BG: Color = Color::Indexed(52);

/// Lane colors cycle by lane index so two neighbouring lanes never share a
/// hue. The graph is still fully readable without them: every lane already
/// draws a distinct glyph (see [`crate::graph_view`]), so this is
/// decoration layered on top of shape, never the carrier of the
/// information (US-066 criterion 2).
const LANES: [Color; 6] = [
    Color::Indexed(33),
    Color::Indexed(215),
    Color::Indexed(170),
    Color::Indexed(78),
    Color::Indexed(80),
    Color::Indexed(203),
];

/// Author initials are tinted from a deterministic hash of the author's
/// identity, so the same person keeps the same color across rows without
/// any avatar data existing (the mockup's avatar column has no equivalent
/// in a Git repository — see `Theme::author_initial`).
const AUTHORS: [Color; 6] = [
    Color::Indexed(110),
    Color::Indexed(180),
    Color::Indexed(144),
    Color::Indexed(175),
    Color::Indexed(109),
    Color::Indexed(186),
];

/// A semantic role, resolved to a concrete [`Style`] by [`Theme::style`].
/// Naming the role rather than the color at each call site is what keeps
/// the low-color path from having to be re-derived in a dozen places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Ordinary body text.
    Text,
    /// Secondary text: paths, timestamps, counts.
    Muted,
    /// A heading or a value that must stand out without being a state.
    Strong,
    /// The accent hue: branding, active affordances.
    Accent,
    /// A short commit hash.
    Hash,
    /// Box borders.
    Border,
    /// A good/clean/added state.
    Ok,
    /// A caution state (dirty working tree, ahead/behind).
    Warn,
    /// A failure/removed state.
    Danger,
    /// Added diff content.
    DiffAdd,
    /// Removed diff content.
    DiffRemove,
}

/// Where a glyph is being drawn, so [`Theme::icon`] can answer with the
/// right pair without every call site repeating the ASCII fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Commits,
    Branches,
    Changes,
    Stashes,
    Remotes,
    Tags,
    Reflog,
    Diff,
    Blame,
    Repository,
    Clean,
    Dirty,
    Cursor,
    Enter,
    UpDown,
    Ahead,
    Behind,
}

/// Resolves [`Role`]s and [`Icon`]s against the current terminal-capability
/// setting. Cheap to copy; `ui.rs` builds one per frame.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    low_color: bool,
}

impl Theme {
    pub fn new(low_color: bool) -> Self {
        Self { low_color }
    }

    pub fn low_color(self) -> bool {
        self.low_color
    }

    pub fn style(self, role: Role) -> Style {
        if self.low_color {
            // Without color the only budget left is weight: Strong/Accent
            // keep their emphasis, everything else flattens to plain text
            // because the shape/marker next to it already says what it is.
            return match role {
                Role::Strong | Role::Accent => Style::default().add_modifier(Modifier::BOLD),
                Role::Muted => Style::default().add_modifier(Modifier::DIM),
                _ => Style::default(),
            };
        }
        match role {
            Role::Text => Style::default(),
            Role::Muted => Style::default().fg(MUTED),
            Role::Strong => Style::default().add_modifier(Modifier::BOLD),
            Role::Accent => Style::default()
                .fg(ACCENT_SOFT)
                .add_modifier(Modifier::BOLD),
            Role::Hash => Style::default().fg(HASH),
            Role::Border => Style::default().fg(BORDER),
            Role::Ok => Style::default().fg(OK),
            Role::Warn => Style::default().fg(WARN),
            Role::Danger => Style::default().fg(DANGER),
            Role::DiffAdd => Style::default().fg(ADD_FG).bg(ADD_BG),
            Role::DiffRemove => Style::default().fg(DEL_FG).bg(DEL_BG),
        }
    }

    /// The solid highlight bar the mockup uses for the active nav row, the
    /// selected commit and the current branch. Reverse video carries the
    /// same "this row is the selection" meaning when there is no color, and
    /// every selectable list additionally draws [`Icon::Cursor`] in its own
    /// column, so the selection is never color-only even here.
    pub fn selection(self) -> Style {
        if self.low_color {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
                .bg(ACCENT)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        }
    }

    /// A keycap: the mockup draws each shortcut key in its own little box.
    pub fn key_chip(self) -> Style {
        if self.low_color {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(ACCENT_SOFT)
                .add_modifier(Modifier::BOLD)
        }
    }

    /// Rounded corners are the mockup's signature; `Plain` is the
    /// conservative fallback for a terminal already declaring reduced
    /// capability, since `╭╮╰╯` sit in a less universally mapped part of
    /// Box Drawing than `┌┐└┘`.
    pub fn border_type(self) -> BorderType {
        if self.low_color {
            BorderType::Plain
        } else {
            BorderType::Rounded
        }
    }

    pub fn icon(self, icon: Icon) -> &'static str {
        let (unicode, ascii) = match icon {
            Icon::Commits => ("◉", "o"),
            Icon::Branches => ("↳", "Y"),
            Icon::Changes => ("▦", "#"),
            Icon::Stashes => ("▤", "S"),
            Icon::Remotes => ("⇄", "R"),
            Icon::Tags => ("◆", "T"),
            Icon::Reflog => ("↺", "@"),
            Icon::Diff => ("±", "+"),
            Icon::Blame => ("≡", "="),
            Icon::Repository => ("▣", "*"),
            Icon::Clean => ("✓", "v"),
            Icon::Dirty => ("●", "!"),
            Icon::Cursor => ("▸", ">"),
            Icon::Enter => ("⏎", "Ent"),
            Icon::UpDown => ("↑↓", "^v"),
            Icon::Ahead => ("↑", "+"),
            Icon::Behind => ("↓", "-"),
        };
        if self.low_color {
            ascii
        } else {
            unicode
        }
    }

    /// The status dot the mockup puts beside every branch and activity row.
    /// The *shape* distinguishes the three cases (filled / hollow /
    /// dotted), so the color that goes with it is redundant by design.
    pub fn dot(self, kind: DotKind) -> Span<'static> {
        let (unicode, ascii, role) = match kind {
            DotKind::Current => ("●", "*", Role::Ok),
            DotKind::Local => ("○", "-", Role::Warn),
            DotKind::Remote => ("◌", "~", Role::Accent),
        };
        if self.low_color {
            Span::raw(ascii)
        } else {
            Span::styled(unicode, self.style(role))
        }
    }

    pub fn lane_color(self, lane: usize) -> Style {
        if self.low_color {
            Style::default()
        } else {
            Style::default().fg(LANES[lane % LANES.len()])
        }
    }

    /// A single, deterministic initial standing in for the mockup's avatar
    /// column. There is no avatar data anywhere in a Git repository, and
    /// inventing a picture is not an option, so the column shows the
    /// author's own first letter instead — derived, never fabricated. An
    /// author whose name has no alphanumeric character at all falls back to
    /// `?` rather than rendering whatever byte happened to be first.
    ///
    /// This is the one place repository text reaches a widget without a
    /// [`crate::sanitize`] pass, and it is safe by construction rather than
    /// by convention: the only character that can ever be emitted passed
    /// `char::is_alphanumeric`, which no escape or control byte does, and
    /// `email` is hashed for a hue but never rendered. SAD §33's rule is
    /// about escape sequences reaching the terminal; none can get here.
    pub fn author_initial(self, name: &str, email: &str) -> Span<'static> {
        let initial = name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let style = if self.low_color {
            Style::default()
        } else {
            let seed = email
                .bytes()
                .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b.into()));
            Style::default().fg(AUTHORS[(seed as usize) % AUTHORS.len()])
        };
        Span::styled(initial, style)
    }

    pub fn span(self, text: impl Into<String>, role: Role) -> Span<'static> {
        Span::styled(text.into(), self.style(role))
    }
}

/// Which of the three branch/reference states a [`Theme::dot`] stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotKind {
    Current,
    Local,
    Remote,
}

/// Display width in terminal cells (never byte or `char` count: one CJK
/// codepoint in a branch name occupies two cells and would otherwise push
/// every right-aligned summary on that row one column out).
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| width(s.content.as_ref())).sum()
}

/// Truncates `text` to at most `max` display columns, marking the cut with
/// `…` (`~` without Unicode) so a shortened value is never mistaken for a
/// complete one.
pub fn clip(theme: Theme, text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    let ellipsis = if theme.low_color() { "~" } else { "…" };
    if max <= 1 {
        return ellipsis.chars().take(max).collect();
    }
    let budget = max - 1;
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push_str(ellipsis);
    out
}

/// Lays one list row out as `left … right` across exactly `width` columns —
/// the mockup's recurring shape (icon + label on the left, a count or a
/// timestamp hard against the right edge).
///
/// The right-hand spans are kept whole and the left ones truncated, because
/// the right side is the small, fixed-size fact a glance is aiming at; a
/// half-printed count would be worse than a shortened subject.
pub fn lay_row(
    theme: Theme,
    total: usize,
    left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
) -> Line<'static> {
    let right_w = spans_width(&right);
    let gap = if right.is_empty() { 0 } else { 1 };
    let budget = total.saturating_sub(right_w + gap);
    let (mut spans, used) = truncate_spans(theme, left, budget);
    spans.push(Span::raw(" ".repeat(total.saturating_sub(used + right_w))));
    spans.extend(right);
    Line::from(spans)
}

/// Truncates a span sequence to `max` display columns, returning the kept
/// spans and their total width. Styling is preserved per span, so a clipped
/// row keeps the colors of whatever survived the cut.
pub fn truncate_spans(
    theme: Theme,
    spans: Vec<Span<'static>>,
    max: usize,
) -> (Vec<Span<'static>>, usize) {
    let mut out = Vec::with_capacity(spans.len());
    let mut used = 0usize;
    for span in spans {
        let w = width(span.content.as_ref());
        if used + w <= max {
            used += w;
            out.push(span);
            continue;
        }
        let room = max - used;
        if room > 0 {
            let text = clip(theme, span.content.as_ref(), room);
            used += width(&text);
            out.push(Span::styled(text, span.style));
        }
        break;
    }
    (out, used)
}

/// Repaints every span with `style`, dropping their individual colors.
///
/// Needed because Ratatui patches a span's own style over the row style: a
/// cyan hash span drawn inside the selection bar would keep its cyan
/// foreground against the accent background and become unreadable. The
/// selected row is deliberately monochrome.
pub fn repaint(spans: Vec<Span<'static>>, style: Style) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|s| Span::styled(s.content, style))
        .collect()
}

/// Buckets a commit/stash/reflog timestamp into the mockup's relative
/// wording ("2 hours ago"), or its compact form ("2h") for the narrow
/// right-hand column.
///
/// `now_seconds` is passed in rather than read here so the whole function
/// stays pure and testable; `ui.rs` samples the clock once per frame. A
/// timestamp in the future (a skewed committer clock, which real
/// repositories do contain) is reported as `"now"` rather than a negative
/// age.
pub fn relative_time(now_seconds: i64, then_seconds: i64, long: bool) -> String {
    let delta = now_seconds.saturating_sub(then_seconds);
    if delta < 60 {
        return "now".to_string();
    }
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;

    let (count, short_unit, long_unit) = if delta < HOUR {
        (delta / MINUTE, "m", "minute")
    } else if delta < DAY {
        (delta / HOUR, "h", "hour")
    } else if delta < WEEK {
        (delta / DAY, "d", "day")
    } else if delta < MONTH {
        (delta / WEEK, "w", "week")
    } else if delta < YEAR {
        (delta / MONTH, "mo", "month")
    } else {
        (delta / YEAR, "y", "year")
    };

    if long {
        format!(
            "{count} {long_unit}{} ago",
            if count == 1 { "" } else { "s" }
        )
    } else {
        format!("{count}{short_unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3600;
    const DAY: i64 = 24 * HOUR;

    #[test]
    fn every_icon_has_an_ascii_form_that_is_pure_ascii() {
        let low = Theme::new(true);
        for icon in [
            Icon::Commits,
            Icon::Branches,
            Icon::Changes,
            Icon::Stashes,
            Icon::Remotes,
            Icon::Tags,
            Icon::Reflog,
            Icon::Diff,
            Icon::Blame,
            Icon::Repository,
            Icon::Clean,
            Icon::Dirty,
            Icon::Cursor,
            Icon::Enter,
            Icon::UpDown,
            Icon::Ahead,
            Icon::Behind,
        ] {
            let glyph = low.icon(icon);
            assert!(
                glyph.is_ascii() && !glyph.is_empty(),
                "{icon:?} has no ASCII fallback: {glyph:?}"
            );
        }
    }

    #[test]
    fn low_color_never_emits_a_foreground_or_background_color() {
        let low = Theme::new(true);
        for role in [
            Role::Text,
            Role::Muted,
            Role::Strong,
            Role::Accent,
            Role::Hash,
            Role::Border,
            Role::Ok,
            Role::Warn,
            Role::Danger,
            Role::DiffAdd,
            Role::DiffRemove,
        ] {
            let style = low.style(role);
            assert!(style.fg.is_none(), "{role:?} still sets a foreground");
            assert!(style.bg.is_none(), "{role:?} still sets a background");
        }
        assert!(low.selection().fg.is_none());
        assert!(low.selection().bg.is_none());
        assert!(low.selection().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_three_branch_dots_stay_distinguishable_without_color() {
        let low = Theme::new(true);
        let glyphs: Vec<String> = [DotKind::Current, DotKind::Local, DotKind::Remote]
            .into_iter()
            .map(|k| low.dot(k).content.to_string())
            .collect();
        assert_eq!(glyphs.len(), 3);
        assert_ne!(glyphs[0], glyphs[1]);
        assert_ne!(glyphs[1], glyphs[2]);
        assert_ne!(glyphs[0], glyphs[2]);
    }

    #[test]
    fn an_author_initial_is_derived_from_the_name_never_invented() {
        let theme = Theme::new(false);
        assert_eq!(theme.author_initial("ada lovelace", "a@b").content, "A");
        assert_eq!(theme.author_initial("  グレース", "g@h").content, "グ");
        assert_eq!(theme.author_initial("!!!", "x@y").content, "?");
    }

    #[test]
    fn a_row_places_its_summary_hard_against_the_right_edge() {
        let theme = Theme::new(true);
        let line = lay_row(theme, 20, vec![Span::raw("main")], vec![Span::raw("12")]);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "main              12");
        // 20 columns of content is what was asked for; `lay_row` pads the
        // left side to exactly that and appends the summary after it.
        assert_eq!(width(&text), 20);
    }

    #[test]
    fn a_row_truncates_the_left_side_rather_than_the_summary() {
        let theme = Theme::new(true);
        let line = lay_row(
            theme,
            12,
            vec![Span::raw("a-very-long-branch-name")],
            vec![Span::raw("99")],
        );
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.ends_with("99"), "summary was clipped: {text:?}");
        assert!(text.contains('~'), "no truncation marker: {text:?}");
        assert_eq!(width(&text), 12);
    }

    #[test]
    fn clipping_accounts_for_double_width_characters() {
        let theme = Theme::new(false);
        // Four CJK codepoints occupy eight cells; five columns fit two of
        // them plus the ellipsis, never four.
        assert_eq!(width(&clip(theme, "実装実装", 5)), 5);
    }

    #[test]
    fn relative_time_buckets_match_the_mockup_wording() {
        let now = 1_700_000_000;
        assert_eq!(relative_time(now, now - 5, true), "now");
        assert_eq!(relative_time(now, now - 2 * HOUR, true), "2 hours ago");
        assert_eq!(relative_time(now, now - HOUR, true), "1 hour ago");
        assert_eq!(relative_time(now, now - 2 * HOUR, false), "2h");
        assert_eq!(relative_time(now, now - DAY, false), "1d");
        assert_eq!(relative_time(now, now - 400 * DAY, false), "1y");
    }

    #[test]
    fn a_future_timestamp_reads_as_now_rather_than_a_negative_age() {
        let now = 1_700_000_000;
        assert_eq!(relative_time(now, now + 10 * DAY, true), "now");
    }
}
