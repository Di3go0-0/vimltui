//! # vimltui
//!
//! A self-contained, embeddable Vim editor for [Ratatui](https://ratatui.rs) TUI applications.
//!
//! `vimltui` provides a fully functional Vim editing experience that you can drop into any
//! Ratatui-based terminal application. Each [`VimEditor`] instance owns its own text buffer,
//! cursor, mode, undo/redo history, search state, and registers — completely independent
//! from your application state.
//!
//! ## Features
//!
//! - **Normal / Insert / Visual** modes (Char, Line, Block)
//! - **Operator + Motion** composition (`dw`, `ci"`, `y$`, `>j`, `gUw`, ...)
//! - **f / F / t / T** character find motions
//! - **Search** with `/`, `?`, `n`, `N` and match highlighting
//! - **Dot repeat** (`.`) for last edit
//! - **Undo / Redo** with snapshot stack
//! - **Command mode** (`:w`, `:q`, `:wq`, `:123`)
//! - **Registers** and system clipboard integration
//! - **Text objects** (`iw`, `i"`, `i(`)
//! - **Relative line numbers** in the built-in renderer
//! - **Pluggable syntax highlighting** via the [`SyntaxHighlighter`] trait
//!
//! ## Quick Start
//!
//! ```rust
//! use vimltui::{VimEditor, VimModeConfig};
//!
//! // Create an editor with full editing capabilities
//! let mut editor = VimEditor::new("Hello, Vim!", VimModeConfig::default());
//!
//! // Create a read-only viewer (visual selection only, no insert)
//! let mut viewer = VimEditor::new("Read only content", VimModeConfig::read_only());
//!
//! // Get the current content back
//! let text = editor.content();
//! ```
//!
//! ## Handling Input
//!
//! ```rust,no_run
//! use vimltui::{VimEditor, VimModeConfig, EditorAction};
//! use crossterm::event::KeyEvent;
//!
//! let mut editor = VimEditor::new("", VimModeConfig::default());
//!
//! // In your event loop:
//! // let action = editor.handle_key(key_event);
//! // match action {
//! //     EditorAction::Handled => { /* editor consumed the key */ }
//! //     EditorAction::Unhandled(key) => { /* pass to your app's handler */ }
//! //     EditorAction::Save => { /* user typed :w */ }
//! //     EditorAction::Close => { /* user typed :q */ }
//! //     EditorAction::ForceClose => { /* user typed :q! */ }
//! //     EditorAction::SaveAndClose => { /* user typed :wq */ }
//! // }
//! ```
//!
//! ## Rendering
//!
//! Use the built-in renderer with your own [`SyntaxHighlighter`]:
//!
//! ```rust
//! use vimltui::{VimTheme, PlainHighlighter, SyntaxHighlighter};
//! use ratatui::style::Color;
//! use ratatui::text::Span;
//!
//! // Built-in PlainHighlighter for no syntax coloring
//! let highlighter = PlainHighlighter;
//!
//! // Or implement your own:
//! struct SqlHighlighter;
//! impl SyntaxHighlighter for SqlHighlighter {
//!     fn highlight_line<'a>(&self, line: &'a str, spans: &mut Vec<Span<'a>>) {
//!         spans.push(Span::raw(line));
//!     }
//! }
//! ```

pub mod editor;
pub mod render;

use crossterm::event::KeyEvent;
use ratatui::style::Color;
use ratatui::text::Span;

// Re-export the primary type for convenience
pub use editor::VimEditor;

/// Vim editing mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Replace,
    Visual(VisualKind),
}

/// Cursor shape hint for renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

/// Visual selection kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualKind {
    Char,
    Line,
    Block,
}

/// Configures which Vim modes are available in an editor instance.
#[derive(Debug, Clone)]
pub struct VimModeConfig {
    pub insert_allowed: bool,
    pub visual_allowed: bool,
}

impl Default for VimModeConfig {
    fn default() -> Self {
        Self {
            insert_allowed: true,
            visual_allowed: true,
        }
    }
}

impl VimModeConfig {
    /// Read-only mode: visual selection is allowed but insert is disabled.
    pub fn read_only() -> Self {
        Self {
            insert_allowed: false,
            visual_allowed: true,
        }
    }
}

/// Actions returned from [`VimEditor::handle_key()`] to inform the parent application.
pub enum EditorAction {
    /// The editor consumed the key — no further action needed.
    Handled,
    /// The editor does not handle this key — bubble up to the parent.
    Unhandled(KeyEvent),
    /// Save buffer (`:w` or `Ctrl+S`).
    Save,
    /// Close buffer (`:q`).
    Close,
    /// Force close without saving (`:q!`).
    ForceClose,
    /// Save and close (`:wq`, `:x`).
    SaveAndClose,
}

/// Leader key (space by default, like modern Neovim setups).
pub const LEADER_KEY: char = ' ';

/// Operator waiting for a motion (e.g., `d` waits for `w` → `dw`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Yank,
    Change,
    Indent,
    Dedent,
    Uppercase,
    Lowercase,
    ToggleCase,
}

/// The range affected by a motion, used by operators.
#[derive(Debug, Clone)]
pub struct MotionRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub linewise: bool,
}

/// Snapshot of editor state for undo/redo.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

/// Register content (for yank/paste).
#[derive(Debug, Clone, Default)]
pub struct Register {
    pub content: String,
    pub linewise: bool,
}

/// State for incremental search (`/` and `?`).
#[derive(Debug, Clone)]
pub struct SearchState {
    pub pattern: String,
    pub forward: bool,
    pub active: bool,
    pub input_buffer: String,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            forward: true,
            active: false,
            input_buffer: String::new(),
        }
    }
}

/// Recorded keystrokes for dot-repeat (`.`).
#[derive(Debug, Clone)]
pub struct EditRecord {
    pub keys: Vec<KeyEvent>,
}

/// Temporary highlight for yanked text (like Neovim's `vim.highlight.on_yank()`).
#[derive(Debug, Clone)]
pub struct YankHighlight {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub linewise: bool,
    pub created_at: std::time::Instant,
}

impl YankHighlight {
    /// Duration the highlight stays visible.
    const DURATION_MS: u128 = 150;

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_millis() > Self::DURATION_MS
    }
}

/// A gutter sign for a specific line, used for diff indicators.
///
/// Consumers populate [`VimEditor::gutter_signs`] with these values.
/// When empty, the gutter renders exactly as before (zero overhead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GutterSign {
    /// Line was added (new) — green `│` and green line number.
    Added,
    /// Line was modified — yellow `│` and yellow line number.
    Modified,
    /// Lines were deleted above this position — red `▲`.
    DeletedAbove,
    /// Lines were deleted below this position — red `▼`.
    DeletedBelow,
}

/// Direction for `f`/`F`/`t`/`T` character find motions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindDirection {
    Forward,
    Backward,
}

/// Theme colors used by the built-in [`render`] module.
///
/// Each application maps its own theme struct to `VimTheme` before calling
/// [`render::render()`].
#[derive(Debug, Clone)]
pub struct VimTheme {
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub border_insert: Color,
    pub editor_bg: Color,
    pub line_nr: Color,
    pub line_nr_active: Color,
    pub visual_bg: Color,
    pub visual_fg: Color,
    pub dim: Color,
    pub accent: Color,
    /// Background for search matches (all occurrences).
    pub search_match_bg: Color,
    /// Background for the current search match (where the cursor is).
    pub search_current_bg: Color,
    /// Foreground for search match text.
    pub search_match_fg: Color,
    /// Background for yank highlight flash.
    pub yank_highlight_bg: Color,
    /// Background for live substitution replacement preview.
    pub substitute_preview_bg: Color,
    /// Color for "added" gutter signs (default: Green).
    pub sign_added: Color,
    /// Color for "modified" gutter signs (default: Yellow).
    pub sign_modified: Color,
    /// Color for "deleted" gutter signs (default: Red).
    pub sign_deleted: Color,
}

/// Trait for language-specific syntax highlighting.
///
/// Implement this for your language (SQL, JSON, YAML, etc.) and pass it to the
/// [`render`] module. See [`PlainHighlighter`] for a no-op reference implementation.
pub trait SyntaxHighlighter {
    /// Highlight a full line and append styled [`Span`]s.
    fn highlight_line<'a>(&self, line: &'a str, spans: &mut Vec<Span<'a>>);

    /// Highlight a segment of a line (used when part of the line has visual selection).
    /// Defaults to [`highlight_line`](SyntaxHighlighter::highlight_line).
    fn highlight_segment<'a>(&self, text: &'a str, spans: &mut Vec<Span<'a>>) {
        self.highlight_line(text, spans);
    }
}

/// No-op highlighter — renders text without any syntax coloring.
pub struct PlainHighlighter;

impl SyntaxHighlighter for PlainHighlighter {
    fn highlight_line<'a>(&self, line: &'a str, spans: &mut Vec<Span<'a>>) {
        if !line.is_empty() {
            spans.push(Span::raw(line));
        }
    }
}

/// Number of lines kept visible above/below the cursor when scrolling.
pub const SCROLLOFF: usize = 3;
