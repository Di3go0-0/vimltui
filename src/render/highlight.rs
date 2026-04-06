use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::{SyntaxHighlighter, VimTheme, VisualKind, YankHighlight};

/// Visual selection range: ((start_row, start_col), (end_row, end_col)).
pub type VisualRange = Option<((usize, usize), (usize, usize))>;

/// Compute the column range that is visually selected on a given line.
pub fn compute_visual(
    line_idx: usize,
    line_len: usize,
    visual_range: &VisualRange,
    visual_kind: &Option<VisualKind>,
) -> Option<(usize, usize)> {
    let ((sr, sc), (er, ec)) = (*visual_range)?;
    let kind = visual_kind.as_ref()?;

    if line_idx < sr || line_idx > er {
        return None;
    }

    match kind {
        VisualKind::Line => Some((0, line_len)),
        VisualKind::Char => {
            if sr == er {
                Some((sc, (ec + 1).min(line_len)))
            } else if line_idx == sr {
                Some((sc, line_len))
            } else if line_idx == er {
                Some((0, (ec + 1).min(line_len)))
            } else {
                Some((0, line_len))
            }
        }
        VisualKind::Block => {
            let left = sc.min(ec);
            let right = (sc.max(ec) + 1).min(line_len);
            if left < right { Some((left, right)) } else { None }
        }
    }
}

/// Compute the column range highlighted by a yank flash on a given line.
pub fn compute_yank(
    line_idx: usize,
    line_len: usize,
    yank_highlight: &Option<YankHighlight>,
) -> Option<(usize, usize)> {
    let h = yank_highlight.as_ref()?;

    if line_idx < h.start_row || line_idx > h.end_row {
        return None;
    }
    if h.linewise {
        return Some((0, line_len));
    }

    if h.start_row == h.end_row {
        Some((h.start_col, h.end_col.min(line_len)))
    } else if line_idx == h.start_row {
        Some((h.start_col, line_len))
    } else if line_idx == h.end_row {
        Some((0, (h.end_col + 1).min(line_len)))
    } else {
        Some((0, line_len))
    }
}

/// Render a line with visual-selection highlighting.
pub fn render_visual<'a>(
    line: &'a str,
    vis_start: usize,
    vis_end: usize,
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    spans: &mut Vec<Span<'a>>,
) {
    let style = Style::default().bg(theme.visual_bg).fg(theme.visual_fg);
    render_range(line, vis_start, vis_end, style, highlighter, spans);
    if line.is_empty() {
        spans.push(Span::styled(" ", style));
    }
}

/// Render a line with yank-flash highlighting.
pub fn render_yank<'a>(
    line: &'a str,
    start: usize,
    end: usize,
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    spans: &mut Vec<Span<'a>>,
) {
    let style = Style::default().bg(theme.yank_highlight_bg);
    render_range(line, start, end, style, highlighter, spans);
    if line.is_empty() {
        spans.push(Span::styled(" ", style));
    }
}

/// Render a line with `:s` preview highlighting (multiple ranges).
pub fn render_preview<'a>(
    line: &'a str,
    highlights: &[(usize, usize)],
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    spans: &mut Vec<Span<'a>>,
) {
    let preview_style = Style::default()
        .bg(theme.substitute_preview_bg)
        .add_modifier(Modifier::BOLD);

    let mut pos = 0;
    for &(hs, he) in highlights {
        let hs = hs.min(line.len());
        let he = he.min(line.len());
        if hs > pos {
            highlighter.highlight_segment(&line[pos..hs], spans);
        }
        if hs < he {
            spans.push(Span::styled(&line[hs..he], preview_style));
        }
        pos = he;
    }
    if pos < line.len() {
        highlighter.highlight_segment(&line[pos..], spans);
    }
}

/// Render a line with search-match highlighting.
///
/// All occurrences get `search_match_bg`; the occurrence at the cursor gets
/// `search_current_bg`.
#[allow(clippy::too_many_arguments)]
pub fn render_search<'a>(
    line: &'a str,
    line_idx: usize,
    cursor_row: usize,
    cursor_col: usize,
    pattern: &str,
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    spans: &mut Vec<Span<'a>>,
) {
    let pattern_lower = pattern.to_lowercase();
    let line_lower = line.to_lowercase();
    let pat_len = pattern_lower.len();

    if pat_len == 0 || line.is_empty() {
        highlighter.highlight_line(line, spans);
        return;
    }

    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut from = 0;
    while let Some(pos) = line_lower[from..].find(&pattern_lower) {
        let start = from + pos;
        matches.push((start, start + pat_len));
        from = start + 1;
        if from >= line_lower.len() {
            break;
        }
    }

    if matches.is_empty() {
        highlighter.highlight_line(line, spans);
        return;
    }

    let match_style = Style::default()
        .fg(theme.search_match_fg)
        .bg(theme.search_match_bg)
        .add_modifier(Modifier::BOLD);
    let current_style = Style::default()
        .fg(theme.search_match_fg)
        .bg(theme.search_current_bg)
        .add_modifier(Modifier::BOLD);

    let mut pos = 0;
    for &(m_start, m_end) in &matches {
        if m_start > pos {
            highlighter.highlight_segment(&line[pos..m_start], spans);
        }
        let is_current = line_idx == cursor_row && m_start == cursor_col;
        let style = if is_current { current_style } else { match_style };
        spans.push(Span::styled(&line[m_start..m_end.min(line.len())], style));
        pos = m_end;
    }
    if pos < line.len() {
        highlighter.highlight_segment(&line[pos..], spans);
    }
}

/// Shared helper: highlight a single `[start..end)` range within a line.
fn render_range<'a>(
    line: &'a str,
    start: usize,
    end: usize,
    style: Style,
    highlighter: &dyn SyntaxHighlighter,
    spans: &mut Vec<Span<'a>>,
) {
    let len = line.len();
    let s = start.min(len);
    let e = end.min(len);

    if s > 0 {
        highlighter.highlight_segment(&line[..s], spans);
    }
    if s < e {
        spans.push(Span::styled(&line[s..e], style));
    }
    if e < len {
        highlighter.highlight_segment(&line[e..], spans);
    }
}
