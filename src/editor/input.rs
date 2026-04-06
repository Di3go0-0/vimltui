use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::VimEditor;
use super::motions::Motion;
use crate::{EditorAction, FindDirection, Operator, Register, VimMode, VisualKind};

enum SubRange {
    Current,
    All,
    Lines(usize, usize),
}

struct SubstituteCmd {
    range: SubRange,
    pattern: String,
    replacement: String,
    global: bool,
    case_insensitive: bool,
}

impl VimEditor {
    /// Main input handler. Returns EditorAction to inform the parent.
    pub fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        // Record macro keys (except the q that stops recording)
        let is_stop_recording = self.recording_macro.is_some()
            && !self.pending_macro_record
            && key.code == KeyCode::Char('q')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(self.mode, VimMode::Normal)
            && !self.command_active
            && !self.search.active;

        if self.recording_macro.is_some() && !is_stop_recording {
            self.macro_buffer.push(key);
        }

        // Command mode (:) takes highest priority
        if self.command_active {
            let action = self.handle_command_input(key);
            self.update_command_line();
            return action;
        }

        // Leader keys are handled globally in events.rs handle_global_leader()
        // before reaching the VimEditor. No leader handling here.

        // Search input mode takes priority
        let action = if self.search.active {
            self.handle_search_input(key)
        } else {
            match &self.mode {
                VimMode::Normal => self.handle_normal(key),
                VimMode::Insert => self.handle_insert(key),
                VimMode::Replace => self.handle_replace(key),
                VimMode::Visual(_) => self.handle_visual(key),
            }
        };

        self.update_command_line();
        self.ensure_cursor_visible();
        action
    }

    // --- Search Input ---

    fn handle_search_input(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.cancel_search();
                EditorAction::Handled
            }
            KeyCode::Enter => {
                self.commit_search();
                EditorAction::Handled
            }
            KeyCode::Backspace => {
                self.search.input_buffer.pop();
                EditorAction::Handled
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(text) = Self::read_system_clipboard() {
                    // Only use first line for search
                    let first_line = text.lines().next().unwrap_or("");
                    self.search.input_buffer.push_str(first_line);
                }
                EditorAction::Handled
            }
            KeyCode::Char(c) if !ctrl => {
                self.search.input_buffer.push(c);
                EditorAction::Handled
            }
            _ => EditorAction::Handled,
        }
    }

    // --- Command Input (:) ---

    fn handle_command_input(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.command_active = false;
                self.command_buffer.clear();
                self.search.pattern.clear();
                self.preview_lines = None;
                self.preview_highlights.clear();
                EditorAction::Handled
            }
            KeyCode::Enter => {
                let cmd = self.command_buffer.clone();
                self.command_active = false;
                self.command_buffer.clear();
                self.search.pattern.clear();
                self.preview_lines = None;
                self.preview_highlights.clear();
                let action = self.execute_command(&cmd);
                self.ensure_cursor_visible();
                action
            }
            KeyCode::Backspace => {
                if self.command_buffer.is_empty() {
                    self.command_active = false;
                    self.search.pattern.clear();
                    self.preview_lines = None;
                    self.preview_highlights.clear();
                } else {
                    self.command_buffer.pop();
                }
                EditorAction::Handled
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(text) = Self::read_system_clipboard() {
                    let first_line = text.lines().next().unwrap_or("");
                    self.command_buffer.push_str(first_line);
                }
                EditorAction::Handled
            }
            KeyCode::Char(c) if !ctrl => {
                self.command_buffer.push(c);
                EditorAction::Handled
            }
            _ => EditorAction::Handled,
        }
    }

    fn execute_command(&mut self, cmd: &str) -> EditorAction {
        let trimmed = cmd.trim();
        // :number -> go to line
        if let Ok(line_num) = trimmed.parse::<usize>() {
            self.move_to_line(line_num);
            return EditorAction::Handled;
        }
        match trimmed {
            "w" => return EditorAction::Save,
            "q" => return EditorAction::Close,
            "q!" => return EditorAction::ForceClose,
            "wq" | "x" => return EditorAction::SaveAndClose,
            "noh" | "nohlsearch" => {
                self.search.pattern.clear();
                return EditorAction::Handled;
            }
            _ => {}
        }

        // Substitution: [range]s/pattern/replacement/[flags]
        if let Some(sub) = Self::parse_substitute(trimmed) {
            self.execute_substitute(sub);
            return EditorAction::Handled;
        }

        EditorAction::Handled
    }

    /// Parse a substitution command like `s/foo/bar/g`, `%s/foo/bar/gi`, `1,5s/foo/bar/`
    fn parse_substitute(cmd: &str) -> Option<SubstituteCmd> {
        let (range, rest) = if let Some(after) = cmd.strip_prefix('%') {
            (SubRange::All, after)
        } else if let Some(comma_pos) = cmd.find(',') {
            // Try to parse N,Ms/...
            let before_comma = &cmd[..comma_pos];
            let after_comma = &cmd[comma_pos + 1..];
            // Find where 's' starts after the range
            if let Some(s_pos) = after_comma.find('s') {
                let end_str = &after_comma[..s_pos];
                if let (Ok(start), Ok(end)) = (before_comma.parse::<usize>(), end_str.parse::<usize>()) {
                    (SubRange::Lines(start, end), &after_comma[s_pos..])
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            (SubRange::Current, cmd)
        };

        if !rest.starts_with('s') || rest.len() < 4 {
            return None;
        }

        let delim = rest.as_bytes()[1] as char;
        if delim.is_alphanumeric() {
            return None;
        }

        // Parse s/pattern/replacement/flags — handle escaped delimiters
        let body = &rest[2..]; // skip 's' and delimiter
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
        // Last part is flags (or the replacement if no trailing delimiter)
        if parts.len() < 2 {
            // Not enough parts
            if parts.len() == 1 {
                // pattern found but no closing delimiter for replacement
                parts.push(current);
                current = String::new();
            } else {
                return None;
            }
        }

        let pattern = parts[0].clone();
        let replacement = parts[1].clone();
        let flags_str = if parts.len() > 2 { &parts[2] } else { &current };

        let global = flags_str.contains('g');
        let case_insensitive = flags_str.contains('i');

        if pattern.is_empty() {
            return None;
        }

        Some(SubstituteCmd {
            range,
            pattern,
            replacement,
            global,
            case_insensitive,
        })
    }

    fn execute_substitute(&mut self, sub: SubstituteCmd) {
        // Build regex with smartcase
        let case_insensitive = sub.case_insensitive
            || Self::is_smartcase_insensitive(&sub.pattern, "");
        let regex_pattern = if case_insensitive {
            format!("(?i){}", sub.pattern)
        } else {
            sub.pattern.clone()
        };

        let re = match regex::Regex::new(&regex_pattern) {
            Ok(r) => r,
            Err(_) => {
                self.command_line = format!("E486: Invalid pattern: {}", sub.pattern);
                return;
            }
        };

        let (start, end) = match sub.range {
            SubRange::Current => (self.cursor_row, self.cursor_row),
            SubRange::All => (0, self.lines.len().saturating_sub(1)),
            SubRange::Lines(s, e) => {
                let start = s.saturating_sub(1).min(self.lines.len().saturating_sub(1));
                let end = e.saturating_sub(1).min(self.lines.len().saturating_sub(1));
                (start, end)
            }
        };

        self.save_undo();
        let mut total_replacements = 0;
        let mut lines_changed = 0;

        for row in start..=end {
            if row >= self.lines.len() { break; }
            let line = &self.lines[row];
            let new_line = if sub.global {
                re.replace_all(line, sub.replacement.as_str()).to_string()
            } else {
                re.replace(line, sub.replacement.as_str()).to_string()
            };
            if new_line != *line {
                let count = if sub.global {
                    re.find_iter(line).count()
                } else {
                    1
                };
                total_replacements += count;
                lines_changed += 1;
                self.lines[row] = new_line;
            }
        }

        if total_replacements > 0 {
            self.modified = true;
            self.command_line = format!(
                "{} substitution{} on {} line{}",
                total_replacements,
                if total_replacements == 1 { "" } else { "s" },
                lines_changed,
                if lines_changed == 1 { "" } else { "s" },
            );
        } else {
            self.command_line = format!("E486: Pattern not found: {}", sub.pattern);
        }
    }

    // --- Normal Mode ---

    fn handle_normal(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle register prefix ("x) — system clipboard is always used
        if self.pending_register {
            self.pending_register = false;
            return EditorAction::Handled;
        }

        // Handle pending find char (f/F/t/T)
        if let Some((direction, before)) = self.pending_find.take() {
            if let KeyCode::Char(c) = key.code {
                self.last_find = Some((direction, before, c));
                if let Some(op) = self.pending_operator.take() {
                    // Operator + find: compute range from cursor to found char, apply operator
                    let motion = match (direction, before) {
                        (FindDirection::Forward, false) => Motion::FindCharForward(c),
                        (FindDirection::Forward, true) => Motion::FindCharBefore(c),
                        (FindDirection::Backward, false) => Motion::FindCharBackward(c),
                        (FindDirection::Backward, true) => Motion::FindCharAfter(c),
                    };
                    let count = self.take_count();
                    self.execute_operator(&op, &motion, count);
                } else {
                    match direction {
                        FindDirection::Forward => self.find_char_forward(c, before),
                        FindDirection::Backward => self.find_char_backward(c, before),
                    }
                }
            }
            return EditorAction::Handled;
        }

        // Handle pending replace (r)
        if self.pending_replace {
            self.pending_replace = false;
            if let KeyCode::Char(c) = key.code {
                let count = self.take_count().max(1);
                self.save_undo();
                for _ in 0..count {
                    if self.cursor_col < self.current_line_len() {
                        self.replace_char(c);
                    }
                }
                // After replacing, cursor stays on last replaced char
                if self.cursor_col > 0 && count > 1 {
                    self.cursor_col -= 1;
                }
            } else {
                self.pending_count = None;
            }
            return EditorAction::Handled;
        }

        // Handle 'z' prefix (zz, zt, zb)
        if self.pending_z {
            self.pending_z = false;
            return self.handle_z_prefix(key);
        }

        // Handle 'g' prefix
        if self.pending_g {
            self.pending_g = false;
            return self.handle_g_prefix(key);
        }

        // Handle pending gc → wait for 'c' to toggle line comment (gcc)
        if self.pending_gc {
            self.pending_gc = false;
            if key.code == KeyCode::Char('c') {
                return EditorAction::ToggleComment;
            }
            return EditorAction::Handled;
        }

        // Handle pending text object (inner/around)
        if let Some(around) = self.pending_text_object.take() {
            return self.handle_text_object_key(key, around);
        }

        // Handle pending mark set (m + char)
        if self.pending_mark {
            self.pending_mark = false;
            if let KeyCode::Char(c) = key.code {
                if c.is_ascii_lowercase() {
                    self.marks.insert(c, (self.cursor_row, self.cursor_col));
                }
            }
            return EditorAction::Handled;
        }

        // Handle pending goto mark (' or `)
        if let Some(exact) = self.pending_goto_mark.take() {
            if let KeyCode::Char(c) = key.code {
                if let Some(&(row, col)) = self.marks.get(&c) {
                    self.cursor_row = row.min(self.lines.len().saturating_sub(1));
                    if exact {
                        self.cursor_col = col.min(self.current_line_len().saturating_sub(1));
                    } else {
                        self.move_to_first_non_blank();
                    }
                }
            }
            return EditorAction::Handled;
        }

        // Handle pending bracket (] or [) + char
        if let Some(bracket) = self.pending_bracket.take() {
            if let KeyCode::Char('d') = key.code {
                if bracket == ']' {
                    self.jump_to_next_diagnostic();
                } else {
                    self.jump_to_prev_diagnostic();
                }
            }
            return EditorAction::Handled;
        }

        // Handle pending macro record (q + char)
        if self.pending_macro_record {
            self.pending_macro_record = false;
            if let KeyCode::Char(c) = key.code {
                if c.is_ascii_lowercase() {
                    self.recording_macro = Some(c);
                    self.macro_buffer.clear();
                }
            }
            return EditorAction::Handled;
        }

        // Handle pending macro play (@ + char)
        if self.pending_macro_play {
            self.pending_macro_play = false;
            if let KeyCode::Char(c) = key.code {
                let reg = if c == '@' {
                    self.last_macro
                } else {
                    Some(c)
                };
                if let Some(r) = reg {
                    self.last_macro = Some(r);
                    if let Some(keys) = self.macro_registers.get(&r).cloned() {
                        for k in keys {
                            self.handle_key(k);
                        }
                    }
                }
            }
            return EditorAction::Handled;
        }

        // Count prefix (digits)
        if let KeyCode::Char(c) = key.code
            && c.is_ascii_digit() && (c != '0' || self.pending_count.is_some()) {
                let digit = c.to_digit(10).unwrap_or(0) as usize;
                let current = self.pending_count.unwrap_or(0);
                self.pending_count = Some(current * 10 + digit);
                return EditorAction::Handled;
            }

        // If we have a pending operator, the next key is a motion
        if self.pending_operator.is_some() {
            return self.handle_operator_motion(key);
        }

        match key.code {
            // --- Movement ---
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                let n = self.take_count();
                self.move_left(n);
                EditorAction::Handled
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                let n = self.take_count();
                self.move_right(n);
                EditorAction::Handled
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                let n = self.take_count();
                self.move_down(n);
                EditorAction::Handled
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                let n = self.take_count();
                self.move_up(n);
                EditorAction::Handled
            }
            KeyCode::Char('w') => {
                let n = self.take_count();
                self.move_word_forward(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('W') => {
                let n = self.take_count();
                self.move_word_forward(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('e') if !ctrl => {
                let n = self.take_count();
                self.move_word_end(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('E') => {
                let n = self.take_count();
                self.move_word_end(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('b') if !ctrl => {
                let n = self.take_count();
                self.move_word_back(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('B') => {
                let n = self.take_count();
                self.move_word_back(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('0') => {
                self.move_to_line_start();
                EditorAction::Handled
            }
            KeyCode::Char('^') => {
                self.move_to_first_non_blank();
                EditorAction::Handled
            }
            KeyCode::Char('$') => {
                self.move_to_line_end();
                EditorAction::Handled
            }
            KeyCode::Char('G') => {
                let count = self.pending_count.take();
                if let Some(n) = count {
                    self.move_to_line(n);
                } else {
                    self.move_to_bottom();
                }
                EditorAction::Handled
            }
            KeyCode::Char('g') => {
                self.pending_g = true;
                EditorAction::Handled
            }

            // --- Scroll ---
            KeyCode::Char('d') if ctrl => {
                self.pending_count = None;
                self.half_page_down();
                EditorAction::Handled
            }
            KeyCode::Char('u') if ctrl => {
                self.pending_count = None;
                self.half_page_up();
                EditorAction::Handled
            }
            KeyCode::Char('f') if ctrl => {
                self.pending_count = None;
                self.full_page_down();
                EditorAction::Handled
            }
            KeyCode::Char('b') if ctrl => {
                self.pending_count = None;
                self.full_page_up();
                EditorAction::Handled
            }
            KeyCode::Char('e') if ctrl => {
                self.pending_count = None;
                self.scroll_line_down();
                EditorAction::Handled
            }
            KeyCode::Char('y') if ctrl => {
                self.pending_count = None;
                self.scroll_line_up();
                EditorAction::Handled
            }

            // --- Screen position ---
            KeyCode::Char('H') => {
                self.pending_count = None;
                self.move_to_screen_top();
                EditorAction::Handled
            }
            KeyCode::Char('M') => {
                self.pending_count = None;
                self.move_to_screen_middle();
                EditorAction::Handled
            }
            KeyCode::Char('L') => {
                self.pending_count = None;
                self.move_to_screen_bottom();
                EditorAction::Handled
            }

            // --- Insert mode entry ---
            KeyCode::Char('i') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.start_recording();
                    self.mode = VimMode::Insert;
                }
                EditorAction::Handled
            }
            KeyCode::Char('a') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.start_recording();
                    self.mode = VimMode::Insert;
                    self.move_right(1);
                }
                EditorAction::Handled
            }
            KeyCode::Char('I') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.start_recording();
                    self.move_to_first_non_blank();
                    self.mode = VimMode::Insert;
                }
                EditorAction::Handled
            }
            KeyCode::Char('A') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.start_recording();
                    self.cursor_col = self.current_line_len();
                    self.mode = VimMode::Insert;
                }
                EditorAction::Handled
            }
            KeyCode::Char('o') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.save_undo();
                    self.start_recording();
                    let indent = {
                        let line = self.current_line();
                        let trimmed = line.trim_start();
                        line[..line.len() - trimmed.len()].to_string()
                    };
                    let row = self.cursor_row + 1;
                    self.lines.insert(row, indent.clone());
                    self.cursor_row = row;
                    self.cursor_col = indent.len();
                    self.mode = VimMode::Insert;
                    self.modified = true;
                }
                EditorAction::Handled
            }
            KeyCode::Char('O') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.save_undo();
                    self.start_recording();
                    let indent = {
                        let line = self.current_line();
                        let trimmed = line.trim_start();
                        line[..line.len() - trimmed.len()].to_string()
                    };
                    self.lines.insert(self.cursor_row, indent.clone());
                    self.cursor_col = indent.len();
                    self.mode = VimMode::Insert;
                    self.modified = true;
                }
                EditorAction::Handled
            }

            // --- Operators ---
            KeyCode::Char('d') => {
                self.pending_operator = Some(Operator::Delete);
                EditorAction::Handled
            }
            KeyCode::Char('y') => {
                self.pending_operator = Some(Operator::Yank);
                EditorAction::Handled
            }
            KeyCode::Char('c') => {
                if self.config.insert_allowed {
                    self.pending_operator = Some(Operator::Change);
                } else {
                    self.pending_operator = Some(Operator::Delete);
                }
                EditorAction::Handled
            }
            KeyCode::Char('>') => {
                self.pending_operator = Some(Operator::Indent);
                EditorAction::Handled
            }
            KeyCode::Char('<') => {
                self.pending_operator = Some(Operator::Dedent);
                EditorAction::Handled
            }

            // --- Single-key edit ---
            KeyCode::Char('x') => {
                let n = self.take_count();
                self.save_undo();
                let mut deleted = String::new();
                for _ in 0..n {
                    if self.cursor_row < self.lines.len()
                        && self.cursor_col < self.lines[self.cursor_row].len()
                    {
                        let ch = self.lines[self.cursor_row].remove(self.cursor_col);
                        deleted.push(ch);
                        self.modified = true;
                    }
                }
                if !deleted.is_empty() {
                    self.unnamed_register = Register {
                        content: deleted.clone(),
                        linewise: false,
                    };
                    self.copy_to_system_clipboard(&deleted);
                    self.clamp_cursor();
                }
                EditorAction::Handled
            }
            KeyCode::Char('X') => {
                if self.cursor_col > 0 {
                    self.save_undo();
                    let n = self.take_count();
                    for _ in 0..n {
                        if self.cursor_col > 0 {
                            self.cursor_col -= 1;
                            self.delete_char_at_cursor();
                        }
                    }
                }
                EditorAction::Handled
            }
            KeyCode::Char('D') => {
                self.pending_count = None;
                let count = 1;
                self.execute_operator(&Operator::Delete, &Motion::LineEnd, count);
                EditorAction::Handled
            }
            KeyCode::Char('C') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.save_undo();
                    self.start_recording();
                    let len = self.current_line_len();
                    if self.cursor_col < len {
                        self.lines[self.cursor_row].truncate(self.cursor_col);
                        self.modified = true;
                    }
                    self.mode = VimMode::Insert;
                }
                EditorAction::Handled
            }
            KeyCode::Char('Y') => {
                self.pending_count = None;
                let count = 1;
                self.execute_operator(&Operator::Yank, &Motion::Line, count);
                EditorAction::Handled
            }
            KeyCode::Char('S') if !ctrl => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.save_undo();
                    self.start_recording();
                    let indent = {
                        let line = self.current_line();
                        let trimmed = line.trim_start();
                        line[..line.len() - trimmed.len()].to_string()
                    };
                    self.lines[self.cursor_row] = indent.clone();
                    self.cursor_col = indent.len();
                    self.mode = VimMode::Insert;
                    self.modified = true;
                }
                EditorAction::Handled
            }
            KeyCode::Char('J') => {
                let n = self.take_count();
                for _ in 0..n {
                    self.join_lines();
                }
                EditorAction::Handled
            }

            // --- Bracket matching ---
            KeyCode::Char('%') => {
                self.pending_count = None;
                self.move_to_matching_bracket();
                EditorAction::Handled
            }

            // --- Undo/Redo ---
            KeyCode::Char('u') if !ctrl => {
                self.pending_count = None;
                self.undo();
                EditorAction::Handled
            }
            KeyCode::Char('r') if ctrl => {
                self.pending_count = None;
                self.redo();
                EditorAction::Handled
            }
            KeyCode::Char('r') => {
                self.pending_replace = true;
                EditorAction::Handled
            }
            KeyCode::Char('R') => {
                if self.config.insert_allowed {
                    self.pending_count = None;
                    self.save_undo();
                    self.start_recording();
                    self.mode = VimMode::Replace;
                }
                EditorAction::Handled
            }
            KeyCode::Char('"') => {
                self.pending_register = true;
                EditorAction::Handled
            }
            KeyCode::Char('p') => {
                self.paste_after();
                EditorAction::Handled
            }
            KeyCode::Char('P') => {
                self.paste_before();
                EditorAction::Handled
            }
            KeyCode::Char('~') => {
                let n = self.take_count();
                for _ in 0..n {
                    self.toggle_case_at_cursor();
                }
                EditorAction::Handled
            }

            // --- Find char (f/F/t/T) ---
            KeyCode::Char('f') => {
                self.pending_count = None;
                self.pending_find = Some((FindDirection::Forward, false));
                EditorAction::Handled
            }
            KeyCode::Char('F') => {
                self.pending_count = None;
                self.pending_find = Some((FindDirection::Backward, false));
                EditorAction::Handled
            }
            KeyCode::Char('t') => {
                self.pending_count = None;
                self.pending_find = Some((FindDirection::Forward, true));
                EditorAction::Handled
            }
            KeyCode::Char('T') => {
                self.pending_count = None;
                self.pending_find = Some((FindDirection::Backward, true));
                EditorAction::Handled
            }
            KeyCode::Char(';') => {
                if let Some((dir, before, ch)) = self.last_find {
                    match dir {
                        FindDirection::Forward => self.find_char_forward(ch, before),
                        FindDirection::Backward => self.find_char_backward(ch, before),
                    }
                }
                EditorAction::Handled
            }
            KeyCode::Char(',') => {
                if let Some((dir, before, ch)) = self.last_find {
                    let rev_dir = match dir {
                        FindDirection::Forward => FindDirection::Backward,
                        FindDirection::Backward => FindDirection::Forward,
                    };
                    match rev_dir {
                        FindDirection::Forward => self.find_char_forward(ch, before),
                        FindDirection::Backward => self.find_char_backward(ch, before),
                    }
                }
                EditorAction::Handled
            }

            // --- Substitute (s) ---
            KeyCode::Char('s') if !ctrl => {
                if self.config.insert_allowed {
                    self.save_undo();
                    self.start_recording();
                    self.delete_char_at_cursor();
                    self.mode = VimMode::Insert;
                }
                EditorAction::Handled
            }

            // --- Search ---
            KeyCode::Char('/') => {
                self.pending_count = None;
                self.start_search(true);
                EditorAction::Handled
            }
            KeyCode::Char('?') => {
                self.pending_count = None;
                self.start_search(false);
                EditorAction::Handled
            }
            KeyCode::Char('n') => {
                self.jump_to_next_match();
                EditorAction::Handled
            }
            KeyCode::Char('N') => {
                self.jump_to_prev_match();
                EditorAction::Handled
            }
            KeyCode::Char('*') => {
                if let Some(word) = self.word_under_cursor() {
                    self.search.pattern = word;
                    self.search.forward = true;
                    self.jump_to_next_match();
                }
                EditorAction::Handled
            }
            KeyCode::Char('#') => {
                if let Some(word) = self.word_under_cursor() {
                    self.search.pattern = word;
                    self.search.forward = false;
                    self.jump_to_prev_match();
                }
                EditorAction::Handled
            }

            // --- Visual mode ---
            KeyCode::Char('v') if ctrl => {
                self.pending_count = None;
                self.enter_visual(VisualKind::Block);
                EditorAction::Handled
            }
            KeyCode::Char('v') => {
                self.pending_count = None;
                self.enter_visual(VisualKind::Char);
                EditorAction::Handled
            }
            KeyCode::Char('V') => {
                self.pending_count = None;
                self.enter_visual(VisualKind::Line);
                EditorAction::Handled
            }

            // --- Repeat ---
            KeyCode::Char('.') => {
                self.repeat_last_edit();
                EditorAction::Handled
            }

            // --- Marks ---
            KeyCode::Char('m') => {
                self.pending_mark = true;
                EditorAction::Handled
            }
            KeyCode::Char('\'') => {
                self.pending_goto_mark = Some(false); // line-wise
                EditorAction::Handled
            }
            KeyCode::Char('`') => {
                self.pending_goto_mark = Some(true); // exact position
                EditorAction::Handled
            }

            // --- Bracket navigation ---
            KeyCode::Char(']') => {
                self.pending_bracket = Some(']');
                EditorAction::Handled
            }
            KeyCode::Char('[') => {
                self.pending_bracket = Some('[');
                EditorAction::Handled
            }

            // --- Macros ---
            KeyCode::Char('q') if !ctrl => {
                if self.recording_macro.is_some() {
                    // Stop recording
                    let reg = self.recording_macro.take().unwrap();
                    let keys = std::mem::take(&mut self.macro_buffer);
                    self.macro_registers.insert(reg, keys);
                } else {
                    // Start recording: wait for register char
                    self.pending_macro_record = true;
                }
                EditorAction::Handled
            }
            KeyCode::Char('@') => {
                self.pending_macro_play = true;
                EditorAction::Handled
            }

            // --- Hover (K) ---
            KeyCode::Char('K') => {
                self.pending_count = None;
                EditorAction::Hover
            }

            // --- z-prefix ---
            KeyCode::Char('z') => {
                self.pending_z = true;
                EditorAction::Handled
            }

            // --- Command mode (:) ---
            KeyCode::Char(':') => {
                self.command_active = true;
                self.command_buffer.clear();
                EditorAction::Handled
            }

            // Space -> pass through to parent as Unhandled
            KeyCode::Char(' ') => EditorAction::Unhandled(key),

            // --- Save buffer ---
            KeyCode::Char('S') if ctrl => EditorAction::Save,

            // --- Execute query ---
            // Query execution is now <leader>Enter, not Ctrl+Enter
            KeyCode::Enter if ctrl => EditorAction::Handled,

            // --- Delete key (same as x) ---
            KeyCode::Delete => {
                let n = self.take_count();
                self.save_undo();
                for _ in 0..n {
                    self.delete_char_at_cursor();
                }
                EditorAction::Handled
            }

            // --- Home/End ---
            KeyCode::Home => {
                self.move_to_line_start();
                EditorAction::Handled
            }
            KeyCode::End => {
                self.move_to_line_end();
                EditorAction::Handled
            }

            // --- Escape clears pending and search highlights ---
            KeyCode::Esc => {
                self.pending_count = None;
                self.pending_operator = None;
                self.pending_g = false;
                self.pending_z = false;
                self.pending_find = None;
                self.pending_replace = false;
                self.pending_text_object = None;
                self.pending_mark = false;
                self.pending_goto_mark = None;
                self.pending_bracket = None;
                self.pending_macro_record = false;
                self.pending_macro_play = false;
                self.search.pattern.clear();
                EditorAction::Unhandled(key)
            }

            _ => EditorAction::Unhandled(key),
        }
    }

    fn handle_g_prefix(&mut self, key: KeyEvent) -> EditorAction {
        match key.code {
            KeyCode::Char('g') => {
                let count = self.pending_count.take();
                if let Some(n) = count {
                    self.move_to_line(n);
                } else {
                    self.move_to_top();
                }
                EditorAction::Handled
            }
            KeyCode::Char('d') => {
                self.pending_count = None;
                EditorAction::GoToDefinition
            }
            KeyCode::Char('c') => {
                // gc in Normal mode: wait for second 'c' → toggle line comment
                self.pending_gc = true;
                EditorAction::Handled
            }
            KeyCode::Char('U') => {
                self.pending_operator = Some(Operator::Uppercase);
                EditorAction::Handled
            }
            KeyCode::Char('u') => {
                self.pending_operator = Some(Operator::Lowercase);
                EditorAction::Handled
            }
            KeyCode::Char('~') => {
                self.pending_operator = Some(Operator::ToggleCase);
                EditorAction::Handled
            }
            _ => {
                self.pending_count = None;
                EditorAction::Handled
            }
        }
    }

    fn handle_z_prefix(&mut self, key: KeyEvent) -> EditorAction {
        match key.code {
            KeyCode::Char('z') => {
                self.scroll_center();
                EditorAction::Handled
            }
            KeyCode::Char('t') => {
                self.scroll_top();
                EditorAction::Handled
            }
            KeyCode::Char('b') => {
                self.scroll_bottom();
                EditorAction::Handled
            }
            _ => {
                self.pending_count = None;
                EditorAction::Handled
            }
        }
    }

    fn handle_text_object_key(&mut self, key: KeyEvent, around: bool) -> EditorAction {
        let op = match self.pending_operator.take() {
            Some(op) => op,
            None => return EditorAction::Handled,
        };
        let count = self.take_count();

        let motion = match key.code {
            KeyCode::Char('w') => {
                if around { Some(Motion::AroundWord) } else { Some(Motion::InnerWord) }
            }
            KeyCode::Char('"') => {
                if around { Some(Motion::AroundQuote('"')) } else { Some(Motion::InnerQuote('"')) }
            }
            KeyCode::Char('\'') => {
                if around { Some(Motion::AroundQuote('\'')) } else { Some(Motion::InnerQuote('\'')) }
            }
            KeyCode::Char('`') => {
                if around { Some(Motion::AroundQuote('`')) } else { Some(Motion::InnerQuote('`')) }
            }
            KeyCode::Char('(') | KeyCode::Char(')') | KeyCode::Char('b') => {
                if around { Some(Motion::AroundParen('(', ')')) } else { Some(Motion::InnerParen('(', ')')) }
            }
            KeyCode::Char('{') | KeyCode::Char('}') | KeyCode::Char('B') => {
                if around { Some(Motion::AroundParen('{', '}')) } else { Some(Motion::InnerParen('{', '}')) }
            }
            KeyCode::Char('[') | KeyCode::Char(']') => {
                if around { Some(Motion::AroundParen('[', ']')) } else { Some(Motion::InnerParen('[', ']')) }
            }
            KeyCode::Char('<') | KeyCode::Char('>') => {
                if around { Some(Motion::AroundParen('<', '>')) } else { Some(Motion::InnerParen('<', '>')) }
            }
            _ => None,
        };

        if let Some(m) = motion {
            self.execute_operator(&op, &m, count);
        }
        EditorAction::Handled
    }

    fn handle_operator_motion(&mut self, key: KeyEvent) -> EditorAction {
        let op = match self.pending_operator.take() {
            Some(op) => op,
            None => return EditorAction::Handled,
        };
        let count = self.take_count();

        // Check for doubled operator (dd, yy, cc, >>, <<)
        let motion = match key.code {
            KeyCode::Char('d') if op == Operator::Delete => Some(Motion::Line),
            KeyCode::Char('y') if op == Operator::Yank => Some(Motion::Line),
            KeyCode::Char('c') if op == Operator::Change => Some(Motion::Line),
            KeyCode::Char('>') if op == Operator::Indent => Some(Motion::Line),
            KeyCode::Char('<') if op == Operator::Dedent => Some(Motion::Line),

            // Motions
            KeyCode::Char('h') | KeyCode::Left => Some(Motion::Left),
            KeyCode::Char('l') | KeyCode::Right => Some(Motion::Right),
            KeyCode::Char('j') | KeyCode::Down => Some(Motion::Down),
            KeyCode::Char('k') | KeyCode::Up => Some(Motion::Up),
            KeyCode::Char('w') => Some(Motion::WordForward),
            KeyCode::Char('W') => Some(Motion::BigWordForward),
            KeyCode::Char('e') => Some(Motion::WordEnd),
            KeyCode::Char('E') => Some(Motion::BigWordEnd),
            KeyCode::Char('b') => Some(Motion::WordBack),
            KeyCode::Char('B') => Some(Motion::BigWordBack),
            KeyCode::Char('0') | KeyCode::Home => Some(Motion::LineStart),
            KeyCode::Char('$') | KeyCode::End => Some(Motion::LineEnd),
            KeyCode::Char('^') => Some(Motion::FirstNonBlank),
            KeyCode::Char('G') => Some(Motion::ToBottom),
            KeyCode::Char('g') => {
                // gg
                self.pending_g = false;
                Some(Motion::ToTop)
            }

            // Find char motions (f/F/t/T) - store operator back and set pending_find
            KeyCode::Char('f') => {
                self.pending_operator = Some(op);
                self.pending_find = Some((FindDirection::Forward, false));
                return EditorAction::Handled;
            }
            KeyCode::Char('F') => {
                self.pending_operator = Some(op);
                self.pending_find = Some((FindDirection::Backward, false));
                return EditorAction::Handled;
            }
            KeyCode::Char('t') => {
                self.pending_operator = Some(op);
                self.pending_find = Some((FindDirection::Forward, true));
                return EditorAction::Handled;
            }
            KeyCode::Char('T') => {
                self.pending_operator = Some(op);
                self.pending_find = Some((FindDirection::Backward, true));
                return EditorAction::Handled;
            }

            // Text objects: iw, i", i(, aw, a", a(
            KeyCode::Char('i') => {
                self.pending_operator = Some(op);
                self.pending_text_object = Some(false); // inner
                return EditorAction::Handled;
            }
            KeyCode::Char('a') => {
                self.pending_operator = Some(op);
                self.pending_text_object = Some(true); // around
                return EditorAction::Handled;
            }

            // % bracket matching as motion
            KeyCode::Char('%') => Some(Motion::MatchBracket),

            KeyCode::Esc => return EditorAction::Handled,
            _ => None,
        };

        if let Some(m) = motion {
            self.execute_operator(&op, &m, count);
        }

        EditorAction::Handled
    }

    // --- Insert Mode ---

    fn handle_insert(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                // Move cursor back one if possible (vim behavior)
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                // If we were in a block insert, replay edits on the other rows
                if self.block_insert.is_some() {
                    let keys = self.recording_edit.clone();
                    self.finish_block_insert(&keys);
                }
                self.stop_recording();
                self.clamp_cursor();
                EditorAction::Handled
            }
            KeyCode::Char('s') if ctrl => {
                self.mode = VimMode::Normal;
                self.stop_recording();
                EditorAction::Save
            }
            // Ctrl-w: delete word back
            KeyCode::Char('w') if ctrl => {
                self.save_undo();
                // Delete backwards to start of word
                if self.cursor_col > 0 {
                    let line = self.lines[self.cursor_row].clone();
                    let chars: Vec<char> = line.chars().collect();
                    let mut col = self.cursor_col;
                    // Skip whitespace
                    while col > 0 && chars[col - 1].is_whitespace() { col -= 1; }
                    // Skip word chars
                    while col > 0 && (chars[col - 1].is_alphanumeric() || chars[col - 1] == '_') { col -= 1; }
                    let byte_start: usize = chars[..col].iter().map(|c| c.len_utf8()).sum();
                    let byte_end: usize = chars[..self.cursor_col].iter().map(|c| c.len_utf8()).sum();
                    self.lines[self.cursor_row] = format!("{}{}", &line[..byte_start], &line[byte_end..]);
                    self.cursor_col = col;
                    self.modified = true;
                }
                self.record_key(key);
                EditorAction::Handled
            }
            // Ctrl-u: delete to start of line
            KeyCode::Char('u') if ctrl => {
                self.save_undo();
                if self.cursor_col > 0 {
                    let line = &self.lines[self.cursor_row];
                    self.lines[self.cursor_row] = line[self.cursor_col..].to_string();
                    self.cursor_col = 0;
                    self.modified = true;
                }
                self.record_key(key);
                EditorAction::Handled
            }
            // Query execution is now <leader>Enter (not available in Insert mode)
            KeyCode::Enter if ctrl => EditorAction::Handled,
            KeyCode::Enter => {
                self.save_undo();
                self.insert_newline();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Backspace => {
                self.save_undo();
                self.backspace();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Delete => {
                self.save_undo();
                self.delete_char_at_cursor();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Home => {
                self.cursor_col = 0;
                EditorAction::Handled
            }
            KeyCode::End => {
                self.cursor_col = self.current_line_len();
                EditorAction::Handled
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                EditorAction::Handled
            }
            KeyCode::Right => {
                if self.cursor_col < self.current_line_len() {
                    self.cursor_col += 1;
                }
                EditorAction::Handled
            }
            KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.clamp_cursor();
                }
                EditorAction::Handled
            }
            KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.clamp_cursor();
                }
                EditorAction::Handled
            }
            KeyCode::Char(c) if !ctrl => {
                self.save_undo();
                self.insert_char(c);
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Tab => {
                self.save_undo();
                // Insert 4 spaces
                for _ in 0..4 {
                    self.insert_char(' ');
                }
                self.record_key(key);
                EditorAction::Handled
            }
            _ => EditorAction::Handled,
        }
    }

    // --- Replace Mode ---

    fn handle_replace(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                self.stop_recording();
                self.clamp_cursor();
                EditorAction::Handled
            }
            KeyCode::Char('s') if ctrl => {
                self.mode = VimMode::Normal;
                self.stop_recording();
                EditorAction::Save
            }
            KeyCode::Enter => {
                self.save_undo();
                self.insert_newline();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Backspace => {
                self.save_undo();
                self.backspace();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Delete => {
                self.save_undo();
                self.delete_char_at_cursor();
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Home => {
                self.cursor_col = 0;
                EditorAction::Handled
            }
            KeyCode::End => {
                self.cursor_col = self.current_line_len();
                EditorAction::Handled
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                EditorAction::Handled
            }
            KeyCode::Right => {
                if self.cursor_col < self.current_line_len() {
                    self.cursor_col += 1;
                }
                EditorAction::Handled
            }
            KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.clamp_cursor();
                }
                EditorAction::Handled
            }
            KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.clamp_cursor();
                }
                EditorAction::Handled
            }
            KeyCode::Char(c) if !ctrl => {
                self.save_undo();
                // Overwrite: delete char at cursor then insert
                if self.cursor_row < self.lines.len()
                    && self.cursor_col < self.lines[self.cursor_row].len()
                {
                    self.lines[self.cursor_row].remove(self.cursor_col);
                }
                self.insert_char(c);
                self.record_key(key);
                EditorAction::Handled
            }
            KeyCode::Tab => {
                self.save_undo();
                for _ in 0..4 {
                    if self.cursor_row < self.lines.len()
                        && self.cursor_col < self.lines[self.cursor_row].len()
                    {
                        self.lines[self.cursor_row].remove(self.cursor_col);
                    }
                    self.insert_char(' ');
                }
                self.record_key(key);
                EditorAction::Handled
            }
            _ => EditorAction::Handled,
        }
    }

    // --- Visual Mode ---

    fn handle_visual(&mut self, key: KeyEvent) -> EditorAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending g → gc for block comment in visual mode
        if self.pending_g {
            self.pending_g = false;
            if key.code == KeyCode::Char('c') {
                if let Some(((sr, _), (er, _))) = self.visual_range() {
                    self.exit_visual();
                    return EditorAction::ToggleBlockComment {
                        start_row: sr,
                        end_row: er,
                    };
                }
            }
            return EditorAction::Handled;
        }

        // Block replace: waiting for the replacement character after 'r'
        if self.pending_replace {
            self.pending_replace = false;
            if let KeyCode::Char(ch) = key.code {
                self.block_replace(ch);
            }
            return EditorAction::Handled;
        }

        // Count prefix (digits) — same as normal mode
        if let KeyCode::Char(c) = key.code
            && c.is_ascii_digit() && (c != '0' || self.pending_count.is_some()) {
                let digit = c.to_digit(10).unwrap_or(0) as usize;
                let current = self.pending_count.unwrap_or(0);
                self.pending_count = Some(current * 10 + digit);
                return EditorAction::Handled;
            }

        match key.code {
            KeyCode::Esc => {
                self.pending_count = None;
                self.exit_visual();
                EditorAction::Handled
            }
            // Movement (updates selection)
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                let n = self.take_count();
                self.move_left(n);
                EditorAction::Handled
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                let n = self.take_count();
                self.move_right(n);
                EditorAction::Handled
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                let n = self.take_count();
                self.move_down(n);
                EditorAction::Handled
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                let n = self.take_count();
                self.move_up(n);
                EditorAction::Handled
            }
            KeyCode::Char('w') => {
                let n = self.take_count();
                self.move_word_forward(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('W') => {
                let n = self.take_count();
                self.move_word_forward(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('b') if !ctrl => {
                let n = self.take_count();
                self.move_word_back(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('B') => {
                let n = self.take_count();
                self.move_word_back(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('e') if !ctrl => {
                let n = self.take_count();
                self.move_word_end(n, false);
                EditorAction::Handled
            }
            KeyCode::Char('E') => {
                let n = self.take_count();
                self.move_word_end(n, true);
                EditorAction::Handled
            }
            KeyCode::Char('0') => {
                self.pending_count = None;
                self.move_to_line_start();
                EditorAction::Handled
            }
            KeyCode::Char('^') => {
                self.pending_count = None;
                self.move_to_first_non_blank();
                EditorAction::Handled
            }
            KeyCode::Char('$') => {
                self.pending_count = None;
                self.move_to_line_end();
                EditorAction::Handled
            }
            KeyCode::Char('G') => {
                let count = self.pending_count.take();
                if let Some(n) = count {
                    self.move_to_line(n);
                } else {
                    self.move_to_bottom();
                }
                EditorAction::Handled
            }
            KeyCode::Char('g') => {
                self.pending_g = true;
                EditorAction::Handled
            }
            KeyCode::Char('d') if ctrl => {
                self.pending_count = None;
                self.half_page_down();
                EditorAction::Handled
            }
            KeyCode::Char('u') if ctrl => {
                self.pending_count = None;
                self.half_page_up();
                EditorAction::Handled
            }
            KeyCode::Char('f') if ctrl => {
                self.pending_count = None;
                self.full_page_down();
                EditorAction::Handled
            }
            KeyCode::Char('b') if ctrl => {
                self.pending_count = None;
                self.full_page_up();
                EditorAction::Handled
            }
            KeyCode::Char('e') if ctrl => {
                self.pending_count = None;
                self.scroll_line_down();
                EditorAction::Handled
            }
            KeyCode::Char('y') if ctrl => {
                self.pending_count = None;
                self.scroll_line_up();
                EditorAction::Handled
            }

            // --- Bracket matching ---
            KeyCode::Char('%') => {
                self.pending_count = None;
                self.move_to_matching_bracket();
                EditorAction::Handled
            }

            // Actions on selection
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.pending_count = None;
                self.visual_delete();
                EditorAction::Handled
            }
            KeyCode::Char('y') => {
                self.pending_count = None;
                self.visual_yank();
                EditorAction::Handled
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.pending_count = None;
                self.visual_paste();
                EditorAction::Handled
            }
            KeyCode::Char('c') => {
                self.pending_count = None;
                if self.config.insert_allowed {
                    self.start_recording();
                    if matches!(self.mode, VimMode::Visual(VisualKind::Block)) {
                        self.block_change_start();
                    } else {
                        self.visual_delete();
                        self.mode = VimMode::Insert;
                    }
                }
                EditorAction::Handled
            }
            KeyCode::Char('I') => {
                self.pending_count = None;
                if self.config.insert_allowed
                    && matches!(self.mode, VimMode::Visual(VisualKind::Block))
                {
                    self.start_recording();
                    self.block_insert_start();
                }
                EditorAction::Handled
            }
            KeyCode::Char('A') => {
                self.pending_count = None;
                if self.config.insert_allowed
                    && matches!(self.mode, VimMode::Visual(VisualKind::Block))
                {
                    self.start_recording();
                    self.block_append_start();
                }
                EditorAction::Handled
            }
            KeyCode::Char('r') => {
                self.pending_count = None;
                if matches!(self.mode, VimMode::Visual(VisualKind::Block)) {
                    self.pending_replace = true;
                }
                EditorAction::Handled
            }
            KeyCode::Char('>') => {
                self.pending_count = None;
                self.visual_indent();
                EditorAction::Handled
            }
            KeyCode::Char('<') => {
                self.pending_count = None;
                self.visual_dedent();
                EditorAction::Handled
            }
            KeyCode::Char('u') if !ctrl => {
                self.pending_count = None;
                self.visual_lowercase();
                EditorAction::Handled
            }
            KeyCode::Char('U') => {
                self.pending_count = None;
                self.visual_uppercase();
                EditorAction::Handled
            }
            KeyCode::Char('~') => {
                self.pending_count = None;
                self.visual_toggle_case();
                EditorAction::Handled
            }
            KeyCode::Char('o') => {
                self.pending_count = None;
                // Swap cursor and anchor
                if let Some(anchor) = self.visual_anchor {
                    self.visual_anchor = Some((self.cursor_row, self.cursor_col));
                    self.cursor_row = anchor.0;
                    self.cursor_col = anchor.1;
                }
                EditorAction::Handled
            }

            // --- Delete key (same as d/x) ---
            KeyCode::Delete => {
                self.pending_count = None;
                self.visual_delete();
                EditorAction::Handled
            }

            // --- Home/End ---
            KeyCode::Home => {
                self.pending_count = None;
                self.move_to_line_start();
                EditorAction::Handled
            }
            KeyCode::End => {
                self.pending_count = None;
                self.move_to_line_end();
                EditorAction::Handled
            }

            // Switch visual sub-mode
            KeyCode::Char('v') if !ctrl => {
                self.pending_count = None;
                match &self.mode {
                    VimMode::Visual(VisualKind::Char) => self.exit_visual(),
                    _ => {
                        let anchor = self.visual_anchor;
                        self.mode = VimMode::Visual(VisualKind::Char);
                        self.visual_anchor = anchor;
                    }
                }
                EditorAction::Handled
            }
            KeyCode::Char('V') => {
                self.pending_count = None;
                match &self.mode {
                    VimMode::Visual(VisualKind::Line) => self.exit_visual(),
                    _ => {
                        let anchor = self.visual_anchor;
                        self.mode = VimMode::Visual(VisualKind::Line);
                        self.visual_anchor = anchor;
                    }
                }
                EditorAction::Handled
            }

            _ => EditorAction::Handled,
        }
    }

    // --- Edit Recording (for . repeat) ---

    fn start_recording(&mut self) {
        self.is_recording = true;
        self.recording_edit.clear();
    }

    fn record_key(&mut self, key: KeyEvent) {
        if self.is_recording {
            self.recording_edit.push(key);
        }
    }

    fn stop_recording(&mut self) {
        if self.is_recording {
            self.is_recording = false;
            if !self.recording_edit.is_empty() {
                self.last_edit = Some(super::EditRecord {
                    keys: self.recording_edit.clone(),
                });
            }
        }
    }

    fn repeat_last_edit(&mut self) {
        if !self.config.insert_allowed {
            return;
        }
        let Some(edit) = self.last_edit.as_ref().cloned() else { return };
        self.save_undo();
        self.mode = VimMode::Insert;
        for key in &edit.keys {
            // Replay insert mode keys
            match key.code {
                KeyCode::Char(c) => self.insert_char(c),
                KeyCode::Enter => self.insert_newline(),
                KeyCode::Backspace => self.backspace(),
                KeyCode::Delete => self.delete_char_at_cursor(),
                KeyCode::Tab => {
                    for _ in 0..4 {
                        self.insert_char(' ');
                    }
                }
                _ => {}
            }
        }
        self.mode = VimMode::Normal;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
        self.clamp_cursor();
    }
}
