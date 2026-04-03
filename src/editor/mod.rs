pub mod input;
pub mod motions;
pub mod operators;
pub mod search;
pub mod visual;

use crate::{
    EditRecord, FindDirection, Operator, Register, SearchState, Snapshot, VimMode, VimModeConfig,
    YankHighlight, SCROLLOFF,
};

/// A self-contained Vim editor instance with its own buffer, cursor, mode, and state.
/// Each view that needs a Vim editor creates its own VimEditor.
pub struct VimEditor {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub mode: VimMode,
    pub config: VimModeConfig,

    // Scroll
    pub scroll_offset: usize,
    pub visible_height: usize,

    // Undo/Redo
    pub undo_stack: Vec<Snapshot>,
    pub redo_stack: Vec<Snapshot>,

    // Registers
    pub unnamed_register: Register,

    // Search
    pub search: SearchState,

    // Visual mode anchor
    pub visual_anchor: Option<(usize, usize)>,

    // Pending operator/count for Normal mode commands
    pub pending_count: Option<usize>,
    pub pending_operator: Option<Operator>,
    pub pending_g: bool,
    pub pending_register: bool, // waiting for register name after "
    pub use_system_clipboard: bool, // next yank/paste uses system clipboard
    pub pending_find: Option<(FindDirection, bool)>, // for f/F/t/T (direction, before_flag)
    pub last_find: Option<(FindDirection, bool, char)>, // for ;/, repeat
    pub pending_replace: bool, // for r command
    pub pending_z: bool, // for z-prefix (zz, zt, zb)
    pub pending_text_object: Option<bool>, // Some(false)=inner, Some(true)=around

    // Repeat
    pub last_edit: Option<EditRecord>,
    pub recording_edit: Vec<crossterm::event::KeyEvent>,
    pub is_recording: bool,

    // Yank highlight
    pub yank_highlight: Option<YankHighlight>,

    // Status
    pub modified: bool,
    pub command_line: String,

    // Command mode (:)
    pub command_active: bool,
    pub command_buffer: String,

    // Live substitution preview
    pub preview_lines: Option<Vec<String>>,
    /// Highlight ranges for replacement text in preview: (row, start_col, end_col)
    pub preview_highlights: Vec<(usize, usize, usize)>,
}

impl VimEditor {
    pub fn new(content: &str, config: VimModeConfig) -> Self {
        let expanded = content.replace('\t', "    ");
        let lines: Vec<String> = if expanded.is_empty() {
            vec![String::new()]
        } else {
            expanded.lines().map(String::from).collect()
        };

        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            mode: VimMode::Normal,
            config,
            scroll_offset: 0,
            visible_height: 20,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            unnamed_register: Register::default(),
            search: SearchState::default(),
            visual_anchor: None,
            pending_count: None,
            pending_operator: None,
            pending_g: false,
            pending_register: false,
            use_system_clipboard: false,
            pending_find: None,
            last_find: None,
            pending_replace: false,
            pending_z: false,
            pending_text_object: None,
            last_edit: None,
            recording_edit: Vec::new(),
            is_recording: false,
            yank_highlight: None,
            modified: false,
            command_line: String::new(),
            command_active: false,
            command_buffer: String::new(),
            preview_lines: None,
            preview_highlights: Vec::new(),
        }
    }

    pub fn new_empty(config: VimModeConfig) -> Self {
        Self::new("", config)
    }

    pub fn set_content(&mut self, content: &str) {
        let expanded = content.replace('\t', "    ");
        self.lines = if expanded.is_empty() {
            vec![String::new()]
        } else {
            expanded.lines().map(String::from).collect()
        };
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.modified = false;
    }

    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Get the visually selected text
    pub fn selected_text(&self) -> Option<String> {
        let ((sr, sc), (er, ec)) = self.visual_range()?;
        let kind = match &self.mode {
            super::VimMode::Visual(k) => k.clone(),
            _ => return None,
        };

        match kind {
            super::VisualKind::Line => {
                Some(self.lines[sr..=er].join("\n"))
            }
            super::VisualKind::Char => {
                if sr == er {
                    let line = &self.lines[sr];
                    let s = sc.min(line.len());
                    let e = (ec + 1).min(line.len());
                    Some(line[s..e].to_string())
                } else {
                    let mut text = String::new();
                    let first = &self.lines[sr];
                    text.push_str(&first[sc.min(first.len())..]);
                    for row in (sr + 1)..er {
                        text.push('\n');
                        text.push_str(&self.lines[row]);
                    }
                    text.push('\n');
                    let last = &self.lines[er];
                    text.push_str(&last[..(ec + 1).min(last.len())]);
                    Some(text)
                }
            }
            super::VisualKind::Block => {
                let left = sc.min(ec);
                let right = sc.max(ec) + 1;
                let mut text = String::new();
                for row in sr..=er {
                    let line = &self.lines[row];
                    let s = left.min(line.len());
                    let e = right.min(line.len());
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&line[s..e]);
                }
                Some(text)
            }
        }
    }

    #[allow(dead_code)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns the cursor shape hint for the current mode.
    pub fn cursor_shape(&self) -> crate::CursorShape {
        if self.pending_replace {
            return crate::CursorShape::Underline;
        }
        match &self.mode {
            VimMode::Normal => crate::CursorShape::Block,
            VimMode::Insert => crate::CursorShape::Bar,
            VimMode::Replace => crate::CursorShape::Underline,
            VimMode::Visual(_) => crate::CursorShape::Block,
        }
    }

    pub fn current_line(&self) -> &str {
        self.lines.get(self.cursor_row).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn current_line_len(&self) -> usize {
        self.current_line().len()
    }

    /// Clamp cursor column to valid range for current line
    pub fn clamp_cursor(&mut self) {
        let max_col = match self.mode {
            VimMode::Insert | VimMode::Replace => self.current_line_len(),
            _ => self.current_line_len().saturating_sub(1).max(0),
        };
        if self.cursor_col > max_col {
            self.cursor_col = max_col;
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len().saturating_sub(1);
        }
    }

    /// Save current state for undo
    pub fn save_undo(&mut self) {
        self.undo_stack.push(Snapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        });
        self.redo_stack.clear();
    }

    /// Undo last change
    pub fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(Snapshot {
                lines: self.lines.clone(),
                cursor_row: self.cursor_row,
                cursor_col: self.cursor_col,
            });
            self.lines = snapshot.lines;
            self.cursor_row = snapshot.cursor_row;
            self.cursor_col = snapshot.cursor_col;
            self.clamp_cursor();
            self.modified = true;
        }
    }

    /// Redo last undone change
    pub fn redo(&mut self) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(Snapshot {
                lines: self.lines.clone(),
                cursor_row: self.cursor_row,
                cursor_col: self.cursor_col,
            });
            self.lines = snapshot.lines;
            self.cursor_row = snapshot.cursor_row;
            self.cursor_col = snapshot.cursor_col;
            self.clamp_cursor();
            self.modified = true;
        }
    }

    /// Ensure scroll keeps cursor visible with scrolloff
    pub fn ensure_cursor_visible(&mut self) {
        let scrolloff = SCROLLOFF.min(self.visible_height / 2);

        if self.cursor_row < self.scroll_offset + scrolloff {
            self.scroll_offset = self.cursor_row.saturating_sub(scrolloff);
        }

        if self.cursor_row + scrolloff >= self.scroll_offset + self.visible_height {
            self.scroll_offset = (self.cursor_row + scrolloff + 1).saturating_sub(self.visible_height);
        }

        let max_offset = self.lines.len().saturating_sub(self.visible_height);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    // --- Insert mode text operations ---

    pub fn insert_char(&mut self, c: char) {
        if self.cursor_row < self.lines.len() {
            let col = self.cursor_col.min(self.lines[self.cursor_row].len());
            self.lines[self.cursor_row].insert(col, c);
            self.cursor_col = col + 1;
            self.modified = true;
        }
    }

    pub fn insert_newline(&mut self) {
        if self.cursor_row < self.lines.len() {
            let col = self.cursor_col.min(self.lines[self.cursor_row].len());
            let indent = {
                let line = &self.lines[self.cursor_row];
                let trimmed = line.trim_start();
                line[..line.len() - trimmed.len()].to_string()
            };
            let rest = self.lines[self.cursor_row][col..].to_string();
            self.lines[self.cursor_row].truncate(col);
            self.cursor_row += 1;
            self.lines
                .insert(self.cursor_row, format!("{}{}", indent, rest));
            self.cursor_col = indent.len();
            self.modified = true;
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let col = self.cursor_col.min(self.lines[self.cursor_row].len());
            if col > 0 {
                self.lines[self.cursor_row].remove(col - 1);
                self.cursor_col = col - 1;
                self.modified = true;
            }
        } else if self.cursor_row > 0 {
            let current_line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current_line);
            self.modified = true;
        }
    }

    // --- Delete operations ---

    pub fn delete_char_at_cursor(&mut self) {
        if self.cursor_row < self.lines.len() {
            let line_len = self.lines[self.cursor_row].len();
            if self.cursor_col < line_len {
                let ch = self.lines[self.cursor_row].remove(self.cursor_col);
                self.unnamed_register = Register {
                    content: ch.to_string(),
                    linewise: false,
                };
                self.modified = true;
                self.clamp_cursor();
            }
        }
    }

    #[allow(dead_code)]
    pub fn delete_line(&mut self, row: usize) -> Option<String> {
        if row < self.lines.len() {
            let line = self.lines.remove(row);
            if self.lines.is_empty() {
                self.lines.push(String::new());
            }
            self.clamp_cursor();
            self.modified = true;
            Some(line)
        } else {
            None
        }
    }

    pub fn delete_lines(&mut self, start: usize, count: usize) -> String {
        let end = (start + count).min(self.lines.len());
        let removed: Vec<String> = self.lines.drain(start..end).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        self.clamp_cursor();
        self.modified = true;
        removed.join("\n")
    }

    pub fn delete_range(&mut self, start_col: usize, end_col: usize, row: usize) -> String {
        if row >= self.lines.len() {
            return String::new();
        }
        let line_len = self.lines[row].len();
        let s = start_col.min(line_len);
        let e = end_col.min(line_len);
        if s >= e {
            return String::new();
        }
        let removed: String = self.lines[row][s..e].to_string();
        self.lines[row] = format!("{}{}", &self.lines[row][..s], &self.lines[row][e..]);
        self.modified = true;
        removed
    }

    // --- Paste ---

    #[allow(dead_code)]
    pub fn paste_after(&mut self) {
        let reg = self.unnamed_register.clone();
        if reg.content.is_empty() {
            return;
        }
        self.save_undo();
        if reg.linewise {
            let new_lines: Vec<String> = reg.content.lines().map(String::from).collect();
            let insert_at = (self.cursor_row + 1).min(self.lines.len());
            for (i, line) in new_lines.into_iter().enumerate() {
                self.lines.insert(insert_at + i, line);
            }
            self.cursor_row = insert_at;
            self.cursor_col = 0;
        } else {
            let col = (self.cursor_col + 1).min(self.lines[self.cursor_row].len());
            self.lines[self.cursor_row].insert_str(col, &reg.content);
            self.cursor_col = col + reg.content.len() - 1;
        }
        self.modified = true;
    }

    #[allow(dead_code)]
    pub fn paste_before(&mut self) {
        let reg = self.unnamed_register.clone();
        if reg.content.is_empty() {
            return;
        }
        self.save_undo();
        if reg.linewise {
            let new_lines: Vec<String> = reg.content.lines().map(String::from).collect();
            for (i, line) in new_lines.into_iter().enumerate() {
                self.lines.insert(self.cursor_row + i, line);
            }
            self.cursor_col = 0;
        } else {
            let col = self.cursor_col.min(self.lines[self.cursor_row].len());
            self.lines[self.cursor_row].insert_str(col, &reg.content);
            self.cursor_col = col + reg.content.len() - 1;
        }
        self.modified = true;
    }

    // --- Join lines ---

    pub fn join_lines(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.save_undo();
            let next_line = self.lines.remove(self.cursor_row + 1);
            let trimmed = next_line.trim_start();
            let join_col = self.lines[self.cursor_row].len();
            if !self.lines[self.cursor_row].is_empty() && !trimmed.is_empty() {
                self.lines[self.cursor_row].push(' ');
                self.cursor_col = join_col;
            } else {
                self.cursor_col = join_col;
            }
            self.lines[self.cursor_row].push_str(trimmed);
            self.modified = true;
        }
    }

    // --- Indentation ---

    pub fn indent_line(&mut self, row: usize) {
        if row < self.lines.len() {
            self.lines[row].insert_str(0, "    ");
            self.modified = true;
        }
    }

    pub fn dedent_line(&mut self, row: usize) {
        if row < self.lines.len() {
            let line = &self.lines[row];
            let spaces = line.len() - line.trim_start().len();
            let remove = spaces.min(4);
            if remove > 0 {
                self.lines[row] = self.lines[row][remove..].to_string();
                self.modified = true;
            }
        }
    }

    /// Get effective count: pending_count or 1
    pub fn take_count(&mut self) -> usize {
        self.pending_count.take().unwrap_or(1)
    }

    /// Update command line based on current mode
    pub fn update_command_line(&mut self) {
        if self.command_active {
            self.command_line = format!(":{}", self.command_buffer);
            // Live preview: highlight substitution pattern + replacement
            self.search.pattern = self
                .extract_substitute_pattern()
                .unwrap_or_default();
            if let Some((lines, hl)) = self.compute_substitute_preview() {
                self.preview_lines = Some(lines);
                self.preview_highlights = hl;
            } else {
                self.preview_lines = None;
                self.preview_highlights.clear();
            }
            return;
        }
        self.command_line = match &self.mode {
            VimMode::Normal => {
                if self.search.active {
                    let prefix = if self.search.forward { "/" } else { "?" };
                    format!("{}{}", prefix, self.search.input_buffer)
                } else if self.pending_operator.is_some() || self.pending_count.is_some() {
                    let mut s = String::new();
                    if let Some(n) = self.pending_count {
                        s.push_str(&n.to_string());
                    }
                    if let Some(op) = &self.pending_operator {
                        s.push(match op {
                            Operator::Delete => 'd',
                            Operator::Yank => 'y',
                            Operator::Change => 'c',
                            Operator::Indent => '>',
                            Operator::Dedent => '<',
                            Operator::Uppercase => 'U',
                            Operator::Lowercase => 'u',
                        });
                    }
                    s
                } else {
                    String::new()
                }
            }
            VimMode::Insert => "-- INSERT --".to_string(),
            VimMode::Replace => "-- REPLACE --".to_string(),
            VimMode::Visual(kind) => {
                let label = match kind {
                    super::VisualKind::Char => "VISUAL",
                    super::VisualKind::Line => "VISUAL LINE",
                    super::VisualKind::Block => "VISUAL BLOCK",
                };
                format!("-- {} --", label)
            }
        };
    }

    /// Extract the search pattern from a partial substitution command in command_buffer.
    fn extract_substitute_pattern(&self) -> Option<String> {
        let (pattern, _, _, _) = self.extract_substitute_parts()?;
        Some(pattern)
    }

    /// Parse partial substitution command, returning (pattern, replacement, range_all, flags).
    /// Handles incomplete input gracefully (e.g., `:s/hol` without closing delimiter).
    fn extract_substitute_parts(&self) -> Option<(String, Option<String>, bool, String)> {
        let cmd = self.command_buffer.trim();

        // Strip range prefix and determine if % (all lines)
        let (all, rest) = if cmd.starts_with('%') {
            (true, &cmd[1..])
        } else if let Some(pos) = cmd.find('s') {
            let prefix = &cmd[..pos];
            if prefix.is_empty() || prefix.chars().all(|c| c.is_ascii_digit() || c == ',') {
                (false, &cmd[pos..])
            } else {
                return None;
            }
        } else {
            return None;
        };

        if !rest.starts_with('s') || rest.len() < 3 {
            return None;
        }

        let delim = rest.as_bytes()[1] as char;
        if delim.is_alphanumeric() {
            return None;
        }

        // Parse: s/pattern/replacement/flags — each part may be incomplete
        let body = &rest[2..];
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut chars = body.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    if next == delim {
                        current.push(next);
                        chars.next();
                        continue;
                    }
                }
                current.push(c);
            } else if c == delim {
                parts.push(current.clone());
                current.clear();
            } else {
                current.push(c);
            }
        }

        let pattern = if let Some(p) = parts.first() {
            if p.is_empty() { return None; }
            p.clone()
        } else if !current.is_empty() {
            // Still typing the pattern (no closing delimiter yet)
            return Some((current, None, all, String::new()));
        } else {
            return None;
        };

        let replacement = if parts.len() >= 2 {
            Some(parts[1].clone())
        } else if !current.is_empty() {
            // Still typing the replacement
            Some(current.clone())
        } else {
            // Just closed the pattern delimiter, replacement is empty so far
            Some(String::new())
        };

        let flags = if parts.len() >= 3 {
            parts[2].clone()
        } else if parts.len() >= 2 {
            current
        } else {
            String::new()
        };

        Some((pattern, replacement, all, flags))
    }

    /// Determine if a pattern should be case-insensitive (smartcase):
    /// all-lowercase → insensitive, any uppercase → sensitive.
    /// The `i` flag forces insensitive regardless.
    fn is_smartcase_insensitive(pattern: &str, flags: &str) -> bool {
        if flags.contains('i') {
            return true;
        }
        // Smartcase: if pattern has no uppercase letters, match case-insensitively
        !pattern.chars().any(|c| c.is_uppercase())
    }

    /// Compute preview lines and highlight ranges for replacement text.
    fn compute_substitute_preview(
        &self,
    ) -> Option<(Vec<String>, Vec<(usize, usize, usize)>)> {
        let (pattern, replacement, all, flags) = self.extract_substitute_parts()?;
        let replacement = replacement?;

        let case_insensitive = Self::is_smartcase_insensitive(&pattern, &flags);
        let global = flags.contains('g');

        let regex_pattern = if case_insensitive {
            format!("(?i){}", pattern)
        } else {
            pattern
        };
        let re = regex::Regex::new(&regex_pattern).ok()?;

        let (start, end) = if all {
            (0, self.lines.len().saturating_sub(1))
        } else {
            (self.cursor_row, self.cursor_row)
        };

        let mut preview = self.lines.clone();
        let mut highlights = Vec::new();

        for row in start..=end.min(preview.len().saturating_sub(1)) {
            let line = &self.lines[row];
            // Build new line and track replacement positions
            let mut new_line = String::new();
            let mut last_end = 0;
            let matches: Vec<_> = re.find_iter(line).collect();
            let match_count = if global { matches.len() } else { matches.len().min(1) };

            for m in matches.iter().take(match_count) {
                new_line.push_str(&line[last_end..m.start()]);
                let rep_start = new_line.len();
                // Expand replacement (handles $1, $2 etc.)
                let expanded = re.replace(m.as_str(), replacement.as_str());
                new_line.push_str(&expanded);
                let rep_end = new_line.len();
                if rep_start < rep_end {
                    highlights.push((row, rep_start, rep_end));
                }
                last_end = m.end();
            }
            new_line.push_str(&line[last_end..]);
            preview[row] = new_line;
        }

        Some((preview, highlights))
    }

    // --- System clipboard ---

    pub fn copy_to_system_clipboard(&self, text: &str) {
        // Try xclip first, then xsel, then wl-copy (Wayland)
        let cmds: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        for (cmd, args) in cmds {
            if let Ok(mut child) = std::process::Command::new(cmd)
                .args(*args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
                return;
            }
        }
    }

    pub fn paste_from_system_clipboard(&mut self) {
        let cmds: &[(&str, &[&str])] = &[
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ];
        for (cmd, args) in cmds {
            if let Ok(output) = std::process::Command::new(cmd)
                .args(*args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output()
                && output.status.success() {
                    if let Ok(text) = String::from_utf8(output.stdout)
                        && !text.is_empty() {
                            self.save_undo();
                            let col = (self.cursor_col + 1).min(self.current_line_len());
                            // Insert text at cursor
                            if text.contains('\n') {
                                let parts: Vec<&str> = text.split('\n').collect();
                                let after = self.lines[self.cursor_row][col..].to_string();
                                self.lines[self.cursor_row].truncate(col);
                                self.lines[self.cursor_row].push_str(parts[0]);
                                for (i, part) in parts[1..].iter().enumerate() {
                                    self.lines.insert(self.cursor_row + 1 + i, part.to_string());
                                }
                                let last_row = self.cursor_row + parts.len() - 1;
                                self.lines[last_row].push_str(&after);
                                self.cursor_row = last_row;
                                self.cursor_col = self.lines[last_row].len() - after.len();
                            } else {
                                self.lines[self.cursor_row].insert_str(col, &text);
                                self.cursor_col = col + text.len() - 1;
                            }
                            self.modified = true;
                        }
                    return;
                }
        }
    }
}
