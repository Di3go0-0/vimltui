mod gutter;
pub mod highlight;

use crossterm::cursor::SetCursorStyle;
use crossterm::ExecutableCommand;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
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

    // Gutter metrics
    let line_count_width = format!("{}", editor.lines.len()).len().max(3);
    let gutter_cfg = editor.gutter.as_ref();
    let has_diagnostics = gutter_cfg.is_some_and(|g| !g.diagnostics.is_empty());
    let has_diff_signs = gutter_cfg.is_some_and(|g| !g.signs.is_empty());
    let num_col_width = gutter::width(line_count_width, has_diagnostics);
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

    let display_lines = editor.preview_lines.as_ref().unwrap_or(&editor.lines);

    // Pre-truncate wide lines
    let truncated_cache: Vec<Option<String>> = (0..content_height)
        .map(|sr| {
            let idx = editor.scroll_offset + sr;
            if idx < display_lines.len() {
                let tw = UnicodeWidthStr::width(display_lines[idx].as_str());
                if tw > available_text_width {
                    Some(truncate_to_width(&display_lines[idx], available_text_width))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let mut rendered_lines: Vec<Line> = Vec::with_capacity(content_height);

    for (screen_row, cached) in truncated_cache.iter().enumerate() {
        let line_idx = editor.scroll_offset + screen_row;

        // Past end-of-file: tilde line
        if line_idx >= display_lines.len() {
            let mut spans = gutter::render_tilde(line_count_width, has_diagnostics, theme, bg_style);
            pad_to_width(&mut spans, num_col_width, full_width, bg_style);
            rendered_lines.push(Line::from(spans));
            continue;
        }

        let is_cursor_line = line_idx == editor.cursor_row && focused;
        let render_text: &str = cached.as_deref().unwrap_or(&display_lines[line_idx]);

        // Gutter spans
        let mut spans: Vec<Span> = if let Some(g) = gutter_cfg {
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

        // Content spans (highlighting)
        let line_visual = highlight::compute_visual(
            line_idx,
            render_text.len(),
            &visual_range,
            &visual_kind,
        );
        let line_yank = highlight::compute_yank(line_idx, render_text.len(), &yank_highlight);
        let line_preview_hl: Vec<(usize, usize)> = editor
            .preview_highlights
            .iter()
            .filter(|(r, _, _)| *r == line_idx)
            .map(|(_, s, e)| (*s, *e))
            .collect();
        let search_pattern = &editor.search.pattern;

        if let Some((vs, ve)) = line_visual {
            highlight::render_visual(render_text, vs, ve, theme, highlighter, &mut spans);
        } else if !line_preview_hl.is_empty() {
            highlight::render_preview(render_text, &line_preview_hl, theme, highlighter, &mut spans);
        } else if let Some((ys, ye)) = line_yank {
            highlight::render_yank(render_text, ys, ye, theme, highlighter, &mut spans);
        } else if !search_pattern.is_empty() {
            highlight::render_search(
                render_text, line_idx, editor.cursor_row, editor.cursor_col,
                search_pattern, theme, highlighter, &mut spans,
            );
        } else {
            highlighter.highlight_line(render_text, &mut spans);
        }

        // Pad to full width
        let used = num_col_width + UnicodeWidthStr::width(render_text);
        if used < full_width {
            spans.push(Span::styled(" ".repeat(full_width - used), bg_style));
        }

        // Cursor position
        if is_cursor_line && focused {
            frame.set_cursor_position(ratatui::layout::Position {
                x: inner.x + (num_col_width + editor.cursor_col) as u16,
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

    let (cmd_text, cmd_style) = if !editor.command_line.is_empty() {
        (editor.command_line.clone(), Style::default().fg(theme.accent))
    } else {
        (
            format!(" {}:{} ", editor.cursor_row + 1, editor.cursor_col + 1),
            Style::default().fg(theme.dim),
        )
    };

    let cmd_used = UnicodeWidthStr::width(cmd_text.as_str());
    let mut cmd_spans = vec![Span::styled(cmd_text, cmd_style)];
    if cmd_used < full_width {
        cmd_spans.push(Span::styled(" ".repeat(full_width - cmd_used), bg_style));
    }

    frame.render_widget(Clear, cmd_area);
    frame.render_widget(
        Paragraph::new(Line::from(cmd_spans)).style(bg_style),
        cmd_area,
    );
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
