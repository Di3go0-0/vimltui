use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::{DiagnosticSign, GutterConfig, GutterSign, VimTheme};

/// Total character width of the gutter column.
///
/// - `has_diagnostics`: adds 2 chars for `[diag][space]` to the left of the number.
/// - `has_diff_signs`: the diff sign replaces the trailing space (no extra width).
pub fn width(line_count_width: usize, has_diagnostics: bool) -> usize {
    let diag_col = if has_diagnostics { 2 } else { 0 };
    line_count_width + 2 + diag_col
}

/// Build gutter spans for a content line.
///
/// Full layout: `[diag][space][number][space][diff_sign]`
///
/// - Diagnostic column only present when `has_diagnostics` is true.
/// - Diff sign replaces the trailing space when `has_diff_signs` is true.
#[allow(clippy::too_many_arguments)]
pub fn render_line<'a>(
    line_idx: usize,
    cursor_row: usize,
    is_cursor_line: bool,
    line_count_width: usize,
    gutter: &GutterConfig,
    has_diagnostics: bool,
    has_diff_signs: bool,
    theme: &VimTheme,
    bg_style: Style,
) -> Vec<Span<'a>> {
    let mut spans = Vec::with_capacity(5);

    let diag = gutter.diagnostics.get(&line_idx);
    let diff = gutter.signs.get(&line_idx);

    // Diagnostic column: [icon][space] — left of number
    if has_diagnostics {
        let (ch, style) = diag_display(diag, gutter, bg_style);
        spans.push(Span::styled(ch, style));
        spans.push(Span::styled(" ", bg_style));
    }

    // Line number
    let line_num = if has_diff_signs {
        // 1 trailing space (diff sign takes the other)
        if is_cursor_line {
            format!("{:>width$} ", line_idx + 1, width = line_count_width)
        } else {
            let distance = line_idx.abs_diff(cursor_row);
            format!("{:>width$} ", distance, width = line_count_width)
        }
    } else {
        // 2 trailing spaces (no diff sign column)
        if is_cursor_line {
            format!("{:>width$}  ", line_idx + 1, width = line_count_width)
        } else {
            let distance = line_idx.abs_diff(cursor_row);
            format!("{:>width$}  ", distance, width = line_count_width)
        }
    };

    let num_style = num_color(diag, diff, gutter, is_cursor_line, theme);
    spans.push(Span::styled(line_num, num_style));

    // Diff sign column: [sign] — right of number, replaces trailing space
    if has_diff_signs {
        let (ch, style) = diff_display(diff, gutter, bg_style);
        spans.push(Span::styled(ch, style));
    }

    spans
}

/// Build gutter spans for a tilde (past-end-of-file) line.
pub fn render_tilde<'a>(
    line_count_width: usize,
    has_diagnostics: bool,
    theme: &VimTheme,
    bg_style: Style,
) -> Vec<Span<'a>> {
    let mut spans = Vec::with_capacity(3);

    if has_diagnostics {
        spans.push(Span::styled("  ", bg_style));
    }

    let prefix = format!("{:>width$}  ", "~", width = line_count_width);
    spans.push(Span::styled(prefix, Style::default().fg(theme.dim)));
    spans
}

/// Diagnostic icon and color for the left column.
fn diag_display(
    diag: Option<&DiagnosticSign>,
    g: &GutterConfig,
    bg_style: Style,
) -> (&'static str, Style) {
    match diag {
        Some(DiagnosticSign::Error) => ("✘", Style::default().fg(g.sign_error)),
        Some(DiagnosticSign::Warning) => ("⚠", Style::default().fg(g.sign_warning)),
        None => (" ", bg_style),
    }
}

/// Diff sign icon and color for the right column.
fn diff_display(
    diff: Option<&GutterSign>,
    g: &GutterConfig,
    bg_style: Style,
) -> (&'static str, Style) {
    match diff {
        Some(GutterSign::Added) => ("│", Style::default().fg(g.sign_added)),
        Some(GutterSign::Modified) => ("│", Style::default().fg(g.sign_modified)),
        Some(GutterSign::DeletedAbove) => ("▲", Style::default().fg(g.sign_deleted)),
        Some(GutterSign::DeletedBelow) => ("▼", Style::default().fg(g.sign_deleted)),
        None => (" ", bg_style),
    }
}

/// Line-number color: diagnostic takes priority, then diff sign, then default.
fn num_color(
    diag: Option<&DiagnosticSign>,
    diff: Option<&GutterSign>,
    g: &GutterConfig,
    is_cursor_line: bool,
    theme: &VimTheme,
) -> Style {
    // Diagnostic color wins
    if let Some(d) = diag {
        return match d {
            DiagnosticSign::Error => Style::default().fg(g.sign_error),
            DiagnosticSign::Warning => Style::default().fg(g.sign_warning),
        };
    }
    // Diff sign color
    if let Some(s) = diff {
        match s {
            GutterSign::Added => return Style::default().fg(g.sign_added),
            GutterSign::Modified => return Style::default().fg(g.sign_modified),
            _ => {}
        }
    }
    // Default
    if is_cursor_line {
        Style::default().fg(theme.line_nr_active).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.line_nr)
    }
}
