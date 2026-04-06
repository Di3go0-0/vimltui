use super::VimEditor;
use crate::{BlockInsertState, Register, VimMode, VisualKind, YankHighlight};

impl VimEditor {
    /// Enter visual mode
    pub fn enter_visual(&mut self, kind: VisualKind) {
        if !self.config.visual_allowed {
            return;
        }
        self.visual_anchor = Some((self.cursor_row, self.cursor_col));
        self.mode = VimMode::Visual(kind);
    }

    /// Get the ordered (start, end) of the visual selection
    pub fn visual_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.visual_anchor?;
        let cursor = (self.cursor_row, self.cursor_col);

        let (start, end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };

        Some((start, end))
    }

    /// Delete visual selection
    pub fn visual_delete(&mut self) {
        if let Some(kind) = self.visual_kind() {
            self.save_undo();

            match kind {
                VisualKind::Line => {
                    if let Some(((sr, _), (er, _))) = self.visual_range() {
                        let text = self.delete_lines(sr, er - sr + 1);
                        self.unnamed_register = Register {
                            content: text,
                            linewise: true,
                        };
                    }
                }
                VisualKind::Char => {
                    if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                        let ec = (ec + 1).min(
                            self.lines
                                .get(er)
                                .map(|l| l.len())
                                .unwrap_or(0),
                        );
                        if sr == er {
                            let text = self.delete_range(sc, ec, sr);
                            self.unnamed_register = Register {
                                content: text,
                                linewise: false,
                            };
                            self.cursor_col = sc;
                        } else {
                            // Multi-line char delete
                            let mut text = String::new();
                            text.push_str(&self.lines[sr][sc..]);
                            for row in (sr + 1)..er {
                                text.push('\n');
                                text.push_str(&self.lines[row]);
                            }
                            text.push('\n');
                            text.push_str(&self.lines[er][..ec.min(self.lines[er].len())]);

                            let after = self.lines[er][ec.min(self.lines[er].len())..].to_string();
                            self.lines[sr].truncate(sc);
                            self.lines[sr].push_str(&after);
                            if er > sr {
                                self.lines.drain((sr + 1)..=er);
                            }

                            self.unnamed_register = Register {
                                content: text,
                                linewise: false,
                            };
                            self.cursor_row = sr;
                            self.cursor_col = sc;
                            self.modified = true;
                        }
                    }
                }
                VisualKind::Block => {
                    // Block delete: remove columns from each line in the range
                    if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                        let left = sc.min(ec);
                        let right = sc.max(ec) + 1;
                        let mut text = String::new();
                        for row in sr..=er.min(self.lines.len().saturating_sub(1)) {
                            let line_len = self.lines[row].len();
                            let l = left.min(line_len);
                            let r = right.min(line_len);
                            if l < r {
                                if !text.is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(&self.lines[row][l..r]);
                                self.lines[row] = format!(
                                    "{}{}",
                                    &self.lines[row][..l],
                                    &self.lines[row][r..]
                                );
                            }
                        }
                        self.unnamed_register = Register {
                            content: text,
                            linewise: false,
                        };
                        self.cursor_row = sr;
                        self.cursor_col = left;
                        self.modified = true;
                    }
                }
            }

            // Always copy deleted text to system clipboard
            self.copy_to_system_clipboard(&self.unnamed_register.content.clone());
            self.exit_visual();
            self.clamp_cursor();
        }
    }

    /// Yank visual selection
    pub fn visual_yank(&mut self) {
        if let Some(kind) = self.visual_kind() {
            match kind {
                VisualKind::Line => {
                    if let Some(((sr, _), (er, _))) = self.visual_range() {
                        let end = er.min(self.lines.len().saturating_sub(1));
                        let text: Vec<&str> = self.lines[sr..=end]
                            .iter()
                            .map(|s| s.as_str())
                            .collect();
                        self.unnamed_register = Register {
                            content: text.join("\n"),
                            linewise: true,
                        };
                    }
                }
                VisualKind::Char => {
                    if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                        let ec = (ec + 1).min(
                            self.lines.get(er).map(|l| l.len()).unwrap_or(0),
                        );
                        if sr == er {
                            let line = &self.lines[sr];
                            let s = sc.min(line.len());
                            let e = ec.min(line.len());
                            self.unnamed_register = Register {
                                content: line[s..e].to_string(),
                                linewise: false,
                            };
                        } else {
                            let mut text = String::new();
                            text.push_str(&self.lines[sr][sc..]);
                            for row in (sr + 1)..er {
                                text.push('\n');
                                text.push_str(&self.lines[row]);
                            }
                            text.push('\n');
                            text.push_str(&self.lines[er][..ec.min(self.lines[er].len())]);
                            self.unnamed_register = Register {
                                content: text,
                                linewise: false,
                            };
                        }
                    }
                }
                VisualKind::Block => {
                    if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                        let left = sc.min(ec);
                        let right = sc.max(ec) + 1;
                        let mut text = String::new();
                        for row in sr..=er.min(self.lines.len().saturating_sub(1)) {
                            let line_len = self.lines[row].len();
                            let l = left.min(line_len);
                            let r = right.min(line_len);
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            if l < r {
                                text.push_str(&self.lines[row][l..r]);
                            }
                        }
                        self.unnamed_register = Register {
                            content: text,
                            linewise: false,
                        };
                    }
                }
            }
            // Always copy to system clipboard
            self.copy_to_system_clipboard(&self.unnamed_register.content.clone());

            // Set yank highlight before exiting visual (we need the range)
            if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                let linewise = matches!(kind, VisualKind::Line);
                self.yank_highlight = Some(YankHighlight {
                    start_row: sr,
                    start_col: sc,
                    end_row: er,
                    end_col: ec,
                    linewise,
                    created_at: std::time::Instant::now(),
                });
            }

            self.exit_visual();
        }
    }

    /// Paste over visual selection (replace selection with register content)
    pub fn visual_paste(&mut self) {
        if self.visual_kind().is_some() {
            // Save the register content BEFORE deleting (delete overwrites it)
            let paste_reg = self.resolve_paste_register();
            if paste_reg.content.is_empty() {
                return;
            }

            // Delete the selection (this sets unnamed_register to deleted text)
            self.visual_delete();

            // Now insert the saved register content at cursor position
            if paste_reg.linewise {
                let new_lines: Vec<String> =
                    paste_reg.content.lines().map(String::from).collect();
                let insert_at = self.cursor_row;
                for (i, line) in new_lines.into_iter().enumerate() {
                    self.lines.insert(insert_at + i, line);
                }
                self.cursor_row = insert_at;
                self.cursor_col = 0;
            } else {
                let col = self.cursor_col.min(self.lines[self.cursor_row].len());
                self.lines[self.cursor_row].insert_str(col, &paste_reg.content);
                self.cursor_col = col + paste_reg.content.len().saturating_sub(1);
            }
            self.modified = true;
        }
    }

    /// Indent visual selection
    pub fn visual_indent(&mut self) {
        if let Some(((sr, _), (er, _))) = self.visual_range() {
            self.save_undo();
            let end = er.min(self.lines.len().saturating_sub(1));
            for row in sr..=end {
                self.indent_line(row);
            }
            self.exit_visual();
        }
    }

    /// Dedent visual selection
    pub fn visual_dedent(&mut self) {
        if let Some(((sr, _), (er, _))) = self.visual_range() {
            self.save_undo();
            let end = er.min(self.lines.len().saturating_sub(1));
            for row in sr..=end {
                self.dedent_line(row);
            }
            self.exit_visual();
        }
    }

    /// Convert visual selection to uppercase
    pub fn visual_uppercase(&mut self) {
        self.visual_transform_case(|s| s.to_uppercase());
    }

    /// Convert visual selection to lowercase
    pub fn visual_lowercase(&mut self) {
        self.visual_transform_case(|s| s.to_lowercase());
    }

    /// Toggle case of visual selection
    pub fn visual_toggle_case(&mut self) {
        self.visual_transform_case(|s| {
            s.chars()
                .map(|c| {
                    if c.is_uppercase() {
                        c.to_lowercase().to_string()
                    } else {
                        c.to_uppercase().to_string()
                    }
                })
                .collect()
        });
    }

    fn visual_transform_case(&mut self, transform: impl Fn(&str) -> String) {
        if let Some(kind) = self.visual_kind() {
            self.save_undo();
            if let Some(((sr, sc), (er, ec))) = self.visual_range() {
                match kind {
                    VisualKind::Line => {
                        let end = er.min(self.lines.len().saturating_sub(1));
                        for row in sr..=end {
                            self.lines[row] = transform(&self.lines[row]);
                        }
                    }
                    VisualKind::Char => {
                        let ec = (ec + 1).min(
                            self.lines.get(er).map(|l| l.len()).unwrap_or(0),
                        );
                        if sr == er {
                            let line = &self.lines[sr];
                            let s = sc.min(line.len());
                            let e = ec.min(line.len());
                            let changed = transform(&line[s..e]);
                            self.lines[sr] = format!("{}{}{}", &line[..s], changed, &line[e..]);
                        } else {
                            // First line
                            let line = &self.lines[sr];
                            let s = sc.min(line.len());
                            self.lines[sr] = format!("{}{}", &line[..s], transform(&line[s..]));
                            // Middle lines
                            for row in (sr + 1)..er {
                                self.lines[row] = transform(&self.lines[row]);
                            }
                            // Last line
                            let line = &self.lines[er];
                            let e = ec.min(line.len());
                            self.lines[er] = format!("{}{}", transform(&line[..e]), &line[e..]);
                        }
                    }
                    VisualKind::Block => {
                        let left = sc.min(ec);
                        let right = sc.max(ec) + 1;
                        for row in sr..=er.min(self.lines.len().saturating_sub(1)) {
                            let line = &self.lines[row];
                            let l = left.min(line.len());
                            let r = right.min(line.len());
                            if l < r {
                                let changed = transform(&line[l..r]);
                                self.lines[row] = format!("{}{}{}", &line[..l], changed, &line[r..]);
                            }
                        }
                    }
                }
                self.modified = true;
            }
            self.exit_visual();
        }
    }

    /// Block insert (I): insert text at the left column of the block on all rows.
    /// Enters insert mode on the first row; on Esc the edits are replayed on the rest.
    pub fn block_insert_start(&mut self) {
        if let Some(((sr, sc), (er, ec))) = self.visual_range() {
            let left = sc.min(ec);
            self.save_undo();
            self.block_insert = Some(BlockInsertState {
                start_row: sr,
                end_row: er.min(self.lines.len().saturating_sub(1)),
                col: left,
            });
            self.cursor_row = sr;
            self.cursor_col = left;
            self.visual_anchor = None;
            self.mode = VimMode::Insert;
        }
    }

    /// Block append (A): insert text after the right column of the block on all rows.
    pub fn block_append_start(&mut self) {
        if let Some(((sr, sc), (er, ec))) = self.visual_range() {
            let right = sc.max(ec) + 1;
            self.save_undo();
            self.block_insert = Some(BlockInsertState {
                start_row: sr,
                end_row: er.min(self.lines.len().saturating_sub(1)),
                col: right,
            });
            self.cursor_row = sr;
            self.cursor_col = right.min(self.lines[sr].len());
            self.visual_anchor = None;
            self.mode = VimMode::Insert;
        }
    }

    /// Block change (c): delete the block columns, then insert at the left column.
    pub fn block_change_start(&mut self) {
        if let Some(((sr, sc), (er, ec))) = self.visual_range() {
            let left = sc.min(ec);
            let right = sc.max(ec) + 1;
            self.save_undo();
            // Delete block columns from each row
            for row in sr..=er.min(self.lines.len().saturating_sub(1)) {
                let line_len = self.lines[row].len();
                let l = left.min(line_len);
                let r = right.min(line_len);
                if l < r {
                    self.lines[row] = format!("{}{}", &self.lines[row][..l], &self.lines[row][r..]);
                }
            }
            self.modified = true;
            self.block_insert = Some(BlockInsertState {
                start_row: sr,
                end_row: er.min(self.lines.len().saturating_sub(1)),
                col: left,
            });
            self.cursor_row = sr;
            self.cursor_col = left.min(self.lines[sr].len());
            self.visual_anchor = None;
            self.mode = VimMode::Insert;
        }
    }

    /// Block replace (r): replace every character in the block with a single character.
    pub fn block_replace(&mut self, ch: char) {
        if let Some(((sr, sc), (er, ec))) = self.visual_range() {
            let left = sc.min(ec);
            let right = sc.max(ec) + 1;
            self.save_undo();
            for row in sr..=er.min(self.lines.len().saturating_sub(1)) {
                let line_len = self.lines[row].len();
                let l = left.min(line_len);
                let r = right.min(line_len);
                if l < r {
                    let replaced: String = self.lines[row][l..r]
                        .chars()
                        .map(|_| ch)
                        .collect();
                    self.lines[row] = format!(
                        "{}{}{}",
                        &self.lines[row][..l],
                        replaced,
                        &self.lines[row][r..]
                    );
                }
            }
            self.modified = true;
            self.cursor_row = sr;
            self.cursor_col = left;
            self.exit_visual();
        }
    }

    /// Finish a block insert: replay the recorded keystrokes on all remaining rows.
    pub fn finish_block_insert(&mut self, keys: &[crossterm::event::KeyEvent]) {
        if let Some(state) = self.block_insert.take() {
            // The first row (state.start_row) was already edited live.
            // Replay the same keystrokes on every other row.
            for row in (state.start_row + 1)..=state.end_row.min(self.lines.len().saturating_sub(1)) {
                let col = state.col.min(self.lines[row].len());
                let mut insert_col = col;
                for key in keys {
                    match key.code {
                        crossterm::event::KeyCode::Char(c) => {
                            if insert_col <= self.lines[row].len() {
                                self.lines[row].insert(insert_col, c);
                                insert_col += 1;
                            }
                        }
                        crossterm::event::KeyCode::Tab => {
                            for _ in 0..4 {
                                if insert_col <= self.lines[row].len() {
                                    self.lines[row].insert(insert_col, ' ');
                                    insert_col += 1;
                                }
                            }
                        }
                        crossterm::event::KeyCode::Backspace => {
                            if insert_col > 0 && insert_col <= self.lines[row].len() {
                                self.lines[row].remove(insert_col - 1);
                                insert_col -= 1;
                            }
                        }
                        crossterm::event::KeyCode::Delete => {
                            if insert_col < self.lines[row].len() {
                                self.lines[row].remove(insert_col);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Exit visual mode
    pub fn exit_visual(&mut self) {
        self.visual_anchor = None;
        self.mode = super::VimMode::Normal;
    }

    fn visual_kind(&self) -> Option<VisualKind> {
        match &self.mode {
            VimMode::Visual(kind) => Some(kind.clone()),
            _ => None,
        }
    }
}
