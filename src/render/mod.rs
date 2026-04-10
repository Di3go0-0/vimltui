mod gutter;
pub mod highlight;

use std::collections::HashMap;

use crossterm::cursor::SetCursorStyle;
use crossterm::ExecutableCommand;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::editor::VimEditor;
use crate::SyntaxHighlighter;
use crate::{VimMode, VimTheme};

pub fn render(
    frame: &mut Frame,
    editor: &mut VimEditor,
    focused: bool,
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    area: Rect,
    title: &str,
) {
    render_with_options(frame, editor, focused, theme, highlighter, area, title, None);
}

#[allow(clippy::too_many_arguments)]
pub fn render_with_options(
    frame: &mut Frame,
    editor: &mut VimEditor,
    focused: bool,
    theme: &VimTheme,
    highlighter: &dyn SyntaxHighlighter,
    area: Rect,
    title: &str,
    border_override: Option<Color>,
) {
    editor.visible_height = area.height.saturating_sub(3) as usize;

    let default_border = if !focused {
        theme.border_unfocused
    } else {
        match editor.mode {
            VimMode::Insert => theme.border_insert,
            _ => theme.border_focused,
        }
    };
    let border_color = border_override.unwrap_or(default_border);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.editor_bg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 {
        return;
    }

    let content_height = inner.height.saturating_sub(1) as usize;
    let full_width = inner.width as usize;
    let bg_style = Style::default().bg(theme.editor_bg);

    // Marks gutter (leftmost, 1 char). Only active while at least one mark
    // is set — the column disappears when the map is empty so the layout
    // matches pre-marks rendering for consumers that never touch marks.
    let has_marks = !editor.marks.is_empty();
    let line_to_mark: HashMap<usize, char> = if has_marks {
        let mut entries: Vec<(char, usize)> = editor
            .marks
            .iter()
            .map(|(c, (r, _))| (*c, *r))
            .collect();
        // Stable ordering so repeated frames always pick the same letter
        // when several marks share a line.
        entries.sort_by_key(|(c, _)| *c);
        let mut m = HashMap::new();
        for (ch, row) in entries {
            m.entry(row).or_insert(ch);
        }
        m
    } else {
        HashMap::new()
    };
    let mark_col_width: usize = if has_marks { 1 } else { 0 };

    // Gutter metrics
    let line_count_width = format!("{}", editor.lines.len()).len().max(3);
    let gutter_cfg = editor.gutter.as_ref();
    let has_diagnostics = gutter_cfg.is_some_and(|g| !g.diagnostics.is_empty());
    let has_diff_signs = gutter_cfg.is_some_and(|g| !g.signs.is_empty());
    let num_col_width = mark_col_width + gutter::width(line_count_width, has_diagnostics);
    let available_text_width = full_width.saturating_sub(num_col_width);

    // Visual selection
    let visual_range = if let VimMode::Visual(_) = &editor.mode {
        editor.visual_range()
    } else {
        None
    };
    let visual_kind = match &editor.mode {
        VimMode::Visual(k) => Some(k.clone()),
        _ => None,
    };

    // Yank flash
    if editor.yank_highlight.as_ref().is_some_and(|h| h.is_expired()) {
        editor.yank_highlight = None;
    }
    let yank_highlight = editor.yank_highlight.clone();

    // Matching bracket
    let match_bracket = if matches!(editor.mode, VimMode::Normal | VimMode::Visual(_)) {
        highlight::find_matching_bracket(&editor.lines, editor.cursor_row, editor.cursor_col)
    } else {
        None
    };

    // Horizontal scroll: keep cursor visible within available_text_width
    if editor.cursor_col >= editor.horizontal_scroll + available_text_width {
        editor.horizontal_scroll = editor.cursor_col.saturating_sub(available_text_width) + 1;
    } else if editor.cursor_col < editor.horizontal_scroll {
        editor.horizontal_scroll = editor.cursor_col;
    }
    let hscroll = editor.horizontal_scroll;
    // Snap hscroll to char boundary once (used for all lines in the loop)
    // Each line may have different char boundaries, so we snap per-line below.

    let display_lines = editor.preview_lines.as_ref().unwrap_or(&editor.lines);

    // Pre-slice lines for horizontal scroll + truncation.
    // Each entry is Some((sliced_string, bytes_skipped)) when slicing was needed.
    let sliced_cache: Vec<Option<(String, usize)>> = (0..content_height)
        .map(|sr| {
            let idx = editor.scroll_offset + sr;
            if idx < display_lines.len() {
                let full_line = &display_lines[idx];
                if hscroll > 0 || UnicodeWidthStr::width(full_line.as_str()) > available_text_width {
                    let skip = snap_to_char_boundary(full_line, hscroll);
                    let sliced = &full_line[skip..];
                    Some((truncate_to_width(sliced, available_text_width), skip))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let mut rendered_lines: Vec<Line> = Vec::with_capacity(content_height);

    for (screen_row, cached) in sliced_cache.iter().enumerate() {
        let line_idx = editor.scroll_offset + screen_row;

        // Past end-of-file: tilde line
        if line_idx >= display_lines.len() {
            let mut spans: Vec<Span> = Vec::new();
            if has_marks {
                spans.push(Span::styled(" ", bg_style));
            }
            spans.extend(gutter::render_tilde(line_count_width, has_diagnostics, theme, bg_style));
            pad_to_width(&mut spans, num_col_width, full_width, bg_style);
            rendered_lines.push(Line::from(spans));
            continue;
        }

        let is_cursor_line = line_idx == editor.cursor_row && focused;
        let (render_text, bytes_skipped): (&str, usize) = match cached {
            Some((s, skip)) => (s.as_str(), *skip),
            None => (display_lines[line_idx].as_str(), 0),
        };

        // Mark column (leftmost)
        let mut spans: Vec<Span> = Vec::new();
        if has_marks {
            if let Some(&mc) = line_to_mark.get(&line_idx) {
                spans.push(Span::styled(
                    mc.to_string(),
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(" ", bg_style));
            }
        }

        // Gutter spans
        let gutter_spans: Vec<Span> = if let Some(g) = gutter_cfg {
            gutter::render_line(
                line_idx,
                editor.cursor_row,
                is_cursor_line,
                line_count_width,
                g,
                has_diagnostics,
                has_diff_signs,
                theme,
                bg_style,
            )
        } else {
            // No gutter config: plain line number
            let line_num = if is_cursor_line {
                format!("{:>width$}  ", line_idx + 1, width = line_count_width)
            } else {
                let distance = line_idx.abs_diff(editor.cursor_row);
                format!("{:>width$}  ", distance, width = line_count_width)
            };
            let num_style = if is_cursor_line {
                Style::default().fg(theme.line_nr_active).add_modifier(ratatui::style::Modifier::BOLD)
            } else {
                Style::default().fg(theme.line_nr)
            };
            vec![Span::styled(line_num, num_style)]
        };
        spans.extend(gutter_spans);

        // Content spans (highlighting) — built separately for bracket overlay
        let mut content_spans: Vec<Span> = Vec::new();

        // Adjust positions for horizontal scroll
        let adj = |pos: usize| -> usize { pos.saturating_sub(bytes_skipped) };

        let line_visual: Option<(usize, usize)> = highlight::compute_visual(
            line_idx,
            render_text.len(),
            &visual_range,
            &visual_kind,
        ).map(|(vs, ve)| (adj(vs), adj(ve)));
        let line_yank = highlight::compute_yank(line_idx, render_text.len(), &yank_highlight)
            .map(|(ys, ye)| (adj(ys), adj(ye)));
        let line_preview_hl: Vec<(usize, usize)> = editor
            .preview_highlights
            .iter()
            .filter(|(r, _, _)| *r == line_idx)
            .map(|(_, s, e)| (adj(*s), adj(*e)))
            .collect();
        let search_pattern = &editor.search.pattern;

        if let Some((vs, ve)) = line_visual {
            highlight::render_visual(render_text, vs, ve, theme, highlighter, &mut content_spans);
        } else if !line_preview_hl.is_empty() {
            highlight::render_preview(render_text, &line_preview_hl, theme, highlighter, &mut content_spans);
        } else if let Some((ys, ye)) = line_yank {
            highlight::render_yank(render_text, ys, ye, theme, highlighter, &mut content_spans);
        } else if !search_pattern.is_empty() {
            highlight::render_search(
                render_text, line_idx, editor.cursor_row, adj(editor.cursor_col),
                search_pattern, theme, highlighter, &mut content_spans,
            );
        } else if bytes_skipped > 0 {
            // Highlight the FULL line so the highlighter sees context (e.g. `--`
            // for comments), then trim spans to the visible portion.
            let full_line = &display_lines[line_idx];
            let mut full_spans: Vec<Span> = Vec::new();
            highlighter.highlight_line(full_line, &mut full_spans);
            trim_spans_to_range(&mut content_spans, &full_spans, full_line, bytes_skipped, render_text.len());
        } else {
            highlighter.highlight_line(render_text, &mut content_spans);
        }

        // Bracket match overlay on content spans only (no gutter interference)
        if let Some((mr, mc)) = match_bracket {
            let bracket_style = Style::default()
                .bg(theme.match_bracket_bg)
                .fg(theme.match_bracket_fg)
                .add_modifier(Modifier::BOLD);
            if line_idx == mr {
                highlight::overlay_bracket_match(&mut content_spans, render_text, adj(mc), bracket_style);
            }
            if line_idx == editor.cursor_row {
                highlight::overlay_bracket_match(
                    &mut content_spans,
                    render_text,
                    adj(editor.cursor_col),
                    bracket_style,
                );
            }
        }

        spans.extend(content_spans);

        // Pad to full width
        let used = num_col_width + UnicodeWidthStr::width(render_text);
        if used < full_width {
            spans.push(Span::styled(" ".repeat(full_width - used), bg_style));
        }

        // Cursor position (adjusted for horizontal scroll)
        if is_cursor_line && focused {
            frame.set_cursor_position(ratatui::layout::Position {
                x: inner.x + (num_col_width + adj(editor.cursor_col)) as u16,
                y: inner.y + screen_row as u16,
            });
        }

        rendered_lines.push(Line::from(spans));
    }

    // Render content
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    frame.render_widget(Clear, content_area);
    frame.render_widget(Paragraph::new(rendered_lines).style(bg_style), content_area);

    // Command line
    render_command_line(frame, editor, theme, inner, full_width, bg_style);

    // Cursor shape
    if focused {
        let cursor_style = match editor.cursor_shape() {
            crate::CursorShape::Block => SetCursorStyle::SteadyBlock,
            crate::CursorShape::Bar => SetCursorStyle::SteadyBar,
            crate::CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
        };
        let _ = std::io::stdout().execute(cursor_style);
    }
}

fn render_command_line(
    frame: &mut Frame,
    editor: &VimEditor,
    theme: &VimTheme,
    inner: Rect,
    full_width: usize,
    bg_style: Style,
) {
    let cmd_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    // Left side: mode / command / search / diagnostic
    let (left_text, left_style) = if !editor.command_line.is_empty() {
        (editor.command_line.clone(), Style::default().fg(theme.accent))
    } else {
        (String::new(), bg_style)
    };

    // Right side: row,col    All/Top/Bot/XX%
    let right_text = format!(
        "{},{}    {}",
        editor.cursor_row + 1,
        editor.cursor_col + 1,
        file_position(editor),
    );

    let left_width = UnicodeWidthStr::width(left_text.as_str());
    let right_width = UnicodeWidthStr::width(right_text.as_str());

    let mut cmd_spans = Vec::new();
    cmd_spans.push(Span::styled(left_text, left_style));

    // Fill gap between left and right
    let gap = full_width.saturating_sub(left_width + right_width);
    if gap > 0 {
        cmd_spans.push(Span::styled(" ".repeat(gap), bg_style));
    }
    cmd_spans.push(Span::styled(right_text, Style::default().fg(theme.dim)));

    frame.render_widget(Clear, cmd_area);
    frame.render_widget(
        Paragraph::new(Line::from(cmd_spans)).style(bg_style),
        cmd_area,
    );
}

/// Compute Vim-style file position indicator: `All`, `Top`, `Bot`, or `XX%`.
fn file_position(editor: &VimEditor) -> String {
    let total = editor.lines.len();
    let visible = editor.visible_height;

    if total <= visible {
        "All".into()
    } else if editor.scroll_offset == 0 {
        "Top".into()
    } else if editor.scroll_offset + visible >= total {
        "Bot".into()
    } else {
        let pct = (editor.cursor_row + 1) * 100 / total;
        format!("{}%", pct)
    }
}

/// Pad spans to fill `full_width` starting from `used` characters.
fn pad_to_width<'a>(spans: &mut Vec<Span<'a>>, used: usize, full_width: usize, bg_style: Style) {
    if used < full_width {
        spans.push(Span::styled(" ".repeat(full_width - used), bg_style));
    }
}

/// Truncate a string to fit within `max_width` display cells.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        width += cw;
        end = i + c.len_utf8();
    }
    s[..end].to_string()
}

/// Extract the visible portion of full-line spans into `out`.
///
/// `full_spans` are spans produced from the full line. This function walks
/// them, skips the first `skip_bytes` bytes, and collects up to `visible_len`
/// bytes of content, preserving the style of each span.
fn trim_spans_to_range<'a>(
    out: &mut Vec<Span<'a>>,
    full_spans: &[Span<'a>],
    full_line: &'a str,
    skip_bytes: usize,
    visible_len: usize,
) {
    let mut byte_pos: usize = 0;
    let end = skip_bytes + visible_len;

    for span in full_spans {
        let span_start = byte_pos;
        let span_end = byte_pos + span.content.len();
        byte_pos = span_end;

        // Entirely before visible range
        if span_end <= skip_bytes {
            continue;
        }
        // Entirely after visible range
        if span_start >= end {
            break;
        }

        // Compute the overlap with [skip_bytes, end)
        let vis_start = span_start.max(skip_bytes);
        let vis_end = span_end.min(end);

        // Map to offsets within the full_line
        if vis_start < vis_end && vis_start < full_line.len() {
            let s = snap_to_char_boundary(full_line, vis_start);
            let e = snap_to_char_boundary(full_line, vis_end);
            if s < e {
                out.push(Span::styled(&full_line[s..e], span.style));
            }
        }
    }
}

/// Snap a byte index to the nearest valid char boundary at or before `idx`.
fn snap_to_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
