use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::{SyntaxHighlighter, VimTheme, VisualKind, YankHighlight};

/// Visual selection range: ((start_row, start_col), (end_row, end_col)).
pub type VisualRange = Option<((usize, usize), (usize, usize))>;

/// Snap a byte index to the nearest valid char boundary at or before `idx`.
fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Snap a byte index to the nearest valid char boundary at or after `idx`.
fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

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
        let hs = floor_char_boundary(line, hs.min(line.len()));
        let he = ceil_char_boundary(line, he.min(line.len()));
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
    if pattern.is_empty() || line.is_empty() {
        highlighter.highlight_line(line, spans);
        return;
    }

    // Build case-insensitive matches directly on the original line
    // to avoid byte-offset mismatches from to_lowercase() length changes.
    let pattern_lower = pattern.to_lowercase();
    let pat_chars: Vec<char> = pattern_lower.chars().collect();

    let mut matches: Vec<(usize, usize)> = Vec::new();
    let line_chars: Vec<(usize, char)> = line.char_indices().collect();

    let mut ci = 0;
    while ci + pat_chars.len() <= line_chars.len() {
        let mut matched = true;
        for (pi, &pc) in pat_chars.iter().enumerate() {
            let lc = line_chars[ci + pi].1.to_lowercase().next().unwrap_or(line_chars[ci + pi].1);
            if lc != pc {
                matched = false;
                break;
            }
        }
        if matched {
            let byte_start = line_chars[ci].0;
            let byte_end = if ci + pat_chars.len() < line_chars.len() {
                line_chars[ci + pat_chars.len()].0
            } else {
                line.len()
            };
            matches.push((byte_start, byte_end));
            ci += 1;
        } else {
            ci += 1;
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
        spans.push(Span::styled(&line[m_start..m_end], style));
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
    let s = floor_char_boundary(line, start.min(len));
    let e = ceil_char_boundary(line, end.min(len));

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

/// Find the matching bracket for the character at (cursor_row, cursor_col).
/// Returns Some((row, col)) if found, None otherwise.
pub fn find_matching_bracket(lines: &[String], cursor_row: usize, cursor_col: usize) -> Option<(usize, usize)> {
    let line = lines.get(cursor_row)?;
    let ch = line.as_bytes().get(cursor_col).copied()?;
    let (target, direction): (u8, i32) = match ch {
        b'(' => (b')', 1),
        b')' => (b'(', -1),
        b'[' => (b']', 1),
        b']' => (b'[', -1),
        b'{' => (b'}', 1),
        b'}' => (b'{', -1),
        _ => return None,
    };

    let mut depth: i32 = 1;
    let mut r = cursor_row;
    let mut c = cursor_col;

    loop {
        if direction > 0 {
            c += 1;
            if c >= lines.get(r).map_or(0, |l| l.len()) {
                r += 1;
                c = 0;
                if r >= lines.len() {
                    return None;
                }
            }
        } else if c == 0 {
            if r == 0 {
                return None;
            }
            r -= 1;
            c = lines.get(r).map_or(0, |l| l.len().saturating_sub(1));
        } else {
            c -= 1;
        }

        let b = lines.get(r).and_then(|l| l.as_bytes().get(c)).copied()?;
        if b == ch {
            depth += 1;
        }
        if b == target {
            depth -= 1;
            if depth == 0 {
                return Some((r, c));
            }
        }
    }
}

/// Overlay bracket-match highlighting on a single character within existing spans.
///
/// Takes an already-built `Vec<Span>` for a line and applies `style` to the
/// character at byte offset `col` by splitting the span that contains it.
pub fn overlay_bracket_match<'a>(spans: &mut Vec<Span<'a>>, line: &'a str, col: usize, style: Style) {
    if col >= line.len() || !line.is_char_boundary(col) {
        return;
    }
    // Find the byte length of the char at `col`
    let char_len = line[col..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    let col_end = col + char_len;

    // Walk spans to find cumulative byte offset where `col` falls
    let mut byte_offset = 0;
    let mut target_idx = None;
    for (i, span) in spans.iter().enumerate() {
        let span_len = span.content.len();
        if byte_offset + span_len > col {
            target_idx = Some(i);
            break;
        }
        byte_offset += span_len;
    }

    let idx = match target_idx {
        Some(i) => i,
        None => return,
    };

    let local_start = col - byte_offset;
    let local_end = (col_end - byte_offset).min(spans[idx].content.len());

    // We need to split the span. To avoid lifetime issues, we rebuild using
    // indices into the original `line` string.
    let span_start_in_line = byte_offset;
    let old_style = spans[idx].style;

    let mut new_spans = Vec::with_capacity(spans.len() + 2);
    new_spans.extend_from_slice(&spans[..idx]);

    let seg_start = span_start_in_line;
    let seg_end = span_start_in_line + spans[idx].content.len();

    // before bracket
    if local_start > 0 {
        new_spans.push(Span::styled(&line[seg_start..seg_start + local_start], old_style));
    }
    // the bracket itself
    new_spans.push(Span::styled(&line[col..col_end.min(seg_end)], style));
    // after bracket
    if local_end < spans[idx].content.len() {
        new_spans.push(Span::styled(&line[seg_start + local_end..seg_end], old_style));
    }

    if idx + 1 < spans.len() {
        new_spans.extend_from_slice(&spans[idx + 1..]);
    }

    *spans = new_spans;
}
