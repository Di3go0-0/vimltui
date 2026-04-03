use super::VimEditor;

impl VimEditor {
    /// Start search mode
    pub fn start_search(&mut self, forward: bool) {
        self.search.active = true;
        self.search.forward = forward;
        self.search.input_buffer.clear();
    }

    /// Commit the search and jump to first match
    pub fn commit_search(&mut self) {
        self.search.active = false;
        self.search.pattern = self.search.input_buffer.clone();
        if !self.search.pattern.is_empty() {
            self.jump_to_next_match();
        }
    }

    /// Cancel search
    pub fn cancel_search(&mut self) {
        self.search.active = false;
        self.search.input_buffer.clear();
    }

    /// Jump to next match (n)
    pub fn jump_to_next_match(&mut self) {
        if self.search.pattern.is_empty() {
            return;
        }
        let smart_lower = !self.search.pattern.chars().any(|c| c.is_uppercase());
        let pattern = if smart_lower {
            self.search.pattern.to_lowercase()
        } else {
            self.search.pattern.clone()
        };

        if self.search.forward {
            self.find_forward(&pattern, smart_lower);
        } else {
            self.find_backward(&pattern, smart_lower);
        }
    }

    /// Jump to previous match (N)
    pub fn jump_to_prev_match(&mut self) {
        if self.search.pattern.is_empty() {
            return;
        }
        let smart_lower = !self.search.pattern.chars().any(|c| c.is_uppercase());
        let pattern = if smart_lower {
            self.search.pattern.to_lowercase()
        } else {
            self.search.pattern.clone()
        };

        if self.search.forward {
            self.find_backward(&pattern, smart_lower);
        } else {
            self.find_forward(&pattern, smart_lower);
        }
    }

    fn line_text<'a>(line: &'a str, case_insensitive: bool, buf: &'a mut String) -> &'a str {
        if case_insensitive {
            *buf = line.to_lowercase();
            buf.as_str()
        } else {
            line
        }
    }

    fn find_forward(&mut self, pattern: &str, ci: bool) {
        let total = self.lines.len();
        let start_col = self.cursor_col + 1;

        let mut buf = String::new();
        let hay = Self::line_text(&self.lines[self.cursor_row], ci, &mut buf);
        if let Some(pos) = hay[start_col.min(hay.len())..].find(pattern) {
            self.cursor_col = start_col.min(hay.len()) + pos;
            self.ensure_cursor_visible();
            return;
        }

        for offset in 1..=total {
            let row = (self.cursor_row + offset) % total;
            let hay = Self::line_text(&self.lines[row], ci, &mut buf);
            if let Some(pos) = hay.find(pattern) {
                self.cursor_row = row;
                self.cursor_col = pos;
                self.ensure_cursor_visible();
                return;
            }
        }
    }

    fn find_backward(&mut self, pattern: &str, ci: bool) {
        let total = self.lines.len();

        if self.cursor_col > 0 {
            let mut buf = String::new();
            let hay = Self::line_text(&self.lines[self.cursor_row], ci, &mut buf);
            let search_area = &hay[..self.cursor_col.min(hay.len())];
            if let Some(pos) = search_area.rfind(pattern) {
                self.cursor_col = pos;
                self.ensure_cursor_visible();
                return;
            }
        }

        for offset in 1..=total {
            let row = (self.cursor_row + total - offset) % total;
            let mut buf = String::new();
            let hay = Self::line_text(&self.lines[row], ci, &mut buf);
            if let Some(pos) = hay.rfind(pattern) {
                self.cursor_row = row;
                self.cursor_col = pos;
                self.ensure_cursor_visible();
                return;
            }
        }
    }
}
