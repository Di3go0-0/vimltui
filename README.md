# vimltui

A self-contained, embeddable Vim editor for [Ratatui](https://ratatui.rs) TUI applications.

[![Crates.io](https://img.shields.io/crates/v/vimltui.svg)](https://crates.io/crates/vimltui)
[![Docs.rs](https://docs.rs/vimltui/badge.svg)](https://docs.rs/vimltui)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Drop a fully functional Vim editor into any Ratatui app. Each `VimEditor` instance owns its own text buffer, cursor, mode, undo/redo history, search, and registers — zero shared state with your application.

## Features

| Category | Commands |
|----------|----------|
| **Modes** | Normal, Insert, Visual (Char / Line / Block) |
| **Motions** | `h` `j` `k` `l` `w` `b` `e` `W` `B` `E` `0` `^` `$` `gg` `G` `f` `F` `t` `T` |
| **Operators** | `d` `c` `y` `>` `<` `gu` `gU` (composable: `dw` `ci"` `y$` `>j` ...) |
| **Text Objects** | `iw` `i"` `i'` `i(` `i{` |
| **Editing** | `x` `s` `r` `~` `o` `O` `p` `P` `u` `Ctrl+R` `.` |
| **Search** | `/` `?` `n` `N` with match highlighting |
| **Command** | `:w` `:q` `:q!` `:wq` `:x` `:123` |
| **Visual Ops** | `d` `y` `>` `<` on selection |
| **Count Prefix** | `3j` `2dw` `5x` |
| **Clipboard** | System clipboard via wl-copy / xclip / xsel |
| **Rendering** | Relative line numbers, search highlights, visual selection, cursor |

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
vimltui = "0.1"
```

## Quick Start

```rust
use vimltui::{VimEditor, VimModeConfig, EditorAction};

// Full editor
let mut editor = VimEditor::new("fn main() {\n    println!(\"hello\");\n}", VimModeConfig::default());

// Read-only viewer (no insert mode)
let mut viewer = VimEditor::new("SELECT * FROM users;", VimModeConfig::read_only());
```

### Handling Input

```rust
use vimltui::EditorAction;

// In your event loop, pass key events to the editor:
let action = editor.handle_key(key_event);

match action {
    EditorAction::Handled => {}                    // Editor consumed the key
    EditorAction::Unhandled(key) => {}             // Bubble up to your app
    EditorAction::Save => {}                       // :w
    EditorAction::Close => {}                      // :q
    EditorAction::ForceClose => {}                 // :q!
    EditorAction::SaveAndClose => {}               // :wq
}
```

### Rendering

Use the built-in renderer with a `VimTheme` and a `SyntaxHighlighter`:

```rust
use vimltui::{VimTheme, PlainHighlighter, render};
use ratatui::style::Color;

let theme = VimTheme {
    border_focused: Color::Blue,
    border_unfocused: Color::DarkGray,
    border_insert: Color::Green,
    editor_bg: Color::Reset,
    line_nr: Color::DarkGray,
    line_nr_active: Color::White,
    visual_bg: Color::DarkGray,
    visual_fg: Color::White,
    dim: Color::DarkGray,
    accent: Color::Cyan,
    search_match_bg: Color::Yellow,
    search_current_bg: Color::Rgb(255, 165, 0),
    search_match_fg: Color::Black,
};

// In your render function:
render::render(frame, &mut editor, true, &theme, &PlainHighlighter, area, "Editor");
```

### Custom Syntax Highlighting

Implement the `SyntaxHighlighter` trait:

```rust
use vimltui::SyntaxHighlighter;
use ratatui::text::Span;
use ratatui::style::{Style, Color, Modifier};

struct SqlHighlighter;

impl SyntaxHighlighter for SqlHighlighter {
    fn highlight_line<'a>(&self, line: &'a str, spans: &mut Vec<Span<'a>>) {
        // Your highlighting logic here
        spans.push(Span::raw(line));
    }
}
```

## Architecture

```
VimEditor (self-contained)
├── lines: Vec<String>           — owned text buffer
├── cursor_row / cursor_col      — cursor position
├── mode: VimMode                — Normal / Insert / Visual
├── undo_stack / redo_stack      — snapshot-based undo
├── search: SearchState          — /pattern with highlights
├── unnamed_register: Register   — yank/delete clipboard
├── pending_operator / count     — operator + motion state machine
└── config: VimModeConfig        — restrict available modes
```

Key events flow through `handle_key()` → internal state machine → `EditorAction` returned to your app. The editor never reaches into your application state.

## Used By

- [restui](https://github.com/Di3go0-0/restui) — TUI HTTP client
- [dbtui](https://github.com/Di3go0-0/dbtui) — TUI database client

## License

MIT - see [LICENSE](LICENSE)
