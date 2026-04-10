# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.11] - 2026-04-09

### Added

- **Bracket matching highlight** — in Normal and Visual modes, when the cursor is on `(`, `)`, `[`, `]`, `{`, or `}`, both the bracket under the cursor and its matching pair are highlighted. New `match_bracket_bg` and `match_bracket_fg` fields in `VimTheme`.

- **Horizontal scrolling** — long lines now scroll horizontally to keep the cursor visible instead of being truncated. New `horizontal_scroll` field on `VimEditor`. The viewport shifts automatically as the cursor moves past the right or left edge.

### Fixed

- **UTF-8 panic in highlight rendering** — string slicing in `render_range`, `render_search`, and `render_preview` could panic on multi-byte characters (e.g., `ÁÉÍÓÚáéíóú`). All slice operations now snap to char boundaries via `floor_char_boundary` / `ceil_char_boundary`. Search highlighting rewrote to match directly on the original string instead of relying on `to_lowercase()` byte offsets.

- **Visual paste with multi-line content** — `visual_paste` used `insert_str` which jammed multi-line content into a single line. Now correctly splits at newlines: first line merges with the current line, middle lines are inserted, last line joins with the remainder.

- **`gg` in Visual mode** — pressing `gg` in Visual mode did nothing because the `pending_g` handler only recognized `gc` (block comment). Now `gg` correctly moves to the top of the file while extending the selection.

- **Syntax highlighting preserved during horizontal scroll** — when the `--` comment prefix was scrolled off-screen, the visible text lost its comment styling. The renderer now highlights the full line and trims spans to the visible range.

## [0.2.1] - 2026-04-08

### Changed

- **Visual `>` / `<` keep the selection active** — `visual_indent` and `visual_dedent` no longer call `exit_visual()` after applying the operation, so pressing `>` (or `<`) repeatedly indents/dedents the same selection by one level each time without re-entering visual mode. This deviates from stock Vim (which exits visual mode after `>`/`<` and forces a `gv` to reselect), but matches the way users actually use these keys: hold the selection, tap `>` until it's at the right indent level, then `Esc`. No public API change — the function signatures are identical; only the behavior of the existing methods changed.

## [0.2.0] - 2026-04-07

### Added

- **Marks gutter column** — when at least one mark is set, a new 1-character column is prepended to the left of the existing gutter (mark → diagnostic icon → line number → diff sign). The mark's letter is rendered in the accent color on the line it points to. The column vanishes the moment the marks map is empty, so consumers that never use marks see **no layout change** at all. This is purely a render-time addition — no public API changed, no `GutterConfig` field added, and the mark state was already tracked in the `marks` HashMap since 0.1.9. When multiple marks land on the same line, the alphabetically-first character is shown.

### Fixed

- **`m{A-Z}` (uppercase marks) silently ignored** — `pending_mark` only accepted `is_ascii_lowercase`, so `mA`/`mB`/etc. did nothing. Now accepts any ASCII letter (`is_ascii_alphabetic`).

- **`Ctrl+d` wasted the bottom half of the screen at EOF** — `half_page_down` capped the scroll at `lines.len() - 1`, which let the last line float up to the top of the viewport and filled the rest with `~` tildes long before the cursor reached the end of the file. The scroll cap is now `lines.len() - visible_height`, so the last line stops exactly at the bottom of the viewport; from there, only the cursor keeps advancing by half-page until it reaches the last line. The screen stays full of real content during fast traversal.

- **`Ctrl+e` "bounced" near the end of the file** — `scroll_line_down` ran `ensure_cursor_visible` after the early-return path without setting the skip flag, so `SCROLLOFF` (3 lines) would snap the scroll back as soon as the last line got close to the top of the viewport. The flag is now set unconditionally on every `Ctrl+e` press, and the scroll cap was lowered to `lines.len() - 3` so the last line can be lifted up to screen row 2 at most — two rows always remain above it. This matches the "lift the line I'm writing on" workflow at EOF without any visual snap-back.

## [0.1.9] - 2026-04-06

### Added

- **Diagnostic system (`Diagnostic` struct, `DiagnosticSeverity` enum)** — diagnostics are rendered to the LEFT of the line number with a separate `[icon][space]` column. When `GutterConfig::diagnostics` is non-empty, the gutter reserves 2 extra characters. Diagnostic color takes priority for the line number.
  - `DiagnosticSeverity::Error` → red `✘`
  - `DiagnosticSeverity::Warning` → yellow `⚠`
  - Optional `message: Option<String>` shown in the command line when the cursor is on a diagnostic line.
  - Colors customizable via `GutterConfig::sign_error` / `sign_warning`.

- **Separate gutter layout for diff signs vs diagnostics** — diff signs (`GutterSign`) render to the RIGHT of the number. Diagnostics render to the LEFT. Both can coexist on the same line and work independently.

  Full layout: `[diagnostic?][space?][number][space][diff_sign?]`

```rust
use std::collections::HashMap;
use vimltui::{Diagnostic, DiagnosticSeverity, GutterConfig, GutterSign};

let mut signs = HashMap::new();
signs.insert(2, GutterSign::Added);
signs.insert(4, GutterSign::Modified);

let mut diagnostics = HashMap::new();
diagnostics.insert(4, Diagnostic {
    severity: DiagnosticSeverity::Error,
    message: Some("expected `;`".into()),
});
diagnostics.insert(9, Diagnostic {
    severity: DiagnosticSeverity::Warning,
    message: Some("unused variable `y`".into()),
});

editor.gutter = Some(GutterConfig {
    signs,
    diagnostics,
    ..Default::default()
});
// Renders as:
//      1  use std::io;
//      3 │fn main() {
// ✘    5 │    let x = todo!();   ← command line shows: "expected `;`"
//      6      ...
// ⚠   10      let y = 42;       ← command line shows: "unused variable `y`"
```

- **Diagnostic navigation (`]d` / `[d`)** — jump to the next/previous line with a diagnostic. Wraps around at the end/beginning of the file.

- **`EditorAction::GoToDefinition` (`gd`)** — returns an action for the consumer to implement go-to-definition navigation.

- **`EditorAction::Hover` (`K`)** — returns an action for the consumer to implement hover/documentation display.

- **Marks (`m` + char, `'` + char, `` ` `` + char)** — set named marks with `ma`..`mz`, jump to mark line with `'a`, jump to exact position with `` `a ``. Marks are local to each editor instance.

- **Macro recording and playback (`q` + char, `@` + char, `@@`)** — record a key sequence with `qa`..`qz`, stop with `q`, replay with `@a`. `@@` replays the last used macro. The command line shows `recording @a` while recording.

### Changed

- **`DiagnosticSign` renamed to `Diagnostic` struct** — diagnostics now use `Diagnostic { severity: DiagnosticSeverity, message: Option<String> }` instead of a plain enum. `DiagnosticSeverity` replaces the old `DiagnosticSign` enum.

- **Render module refactored into submodules** — `render.rs` split into `render/mod.rs` (orchestration), `render/gutter.rs` (sign column + line numbers), and `render/highlight.rs` (visual, search, yank, preview highlighting). No public API changes to `render()` / `render_with_options()`.

## [0.1.8] - 2026-04-06

### Added

- **Visual block editing (`Ctrl+V` block operations)**:
  - `I` (Shift+I) — insert text at the left column of the block; edits on the first line are replayed on all selected rows when pressing Esc.
  - `A` (Shift+A) — append text after the right column of the block; same replay-on-Esc behavior.
  - `c` — delete the block columns and enter insert mode; replacement text is replicated across all rows on Esc.
  - `r` + char — replace every character in the block selection with a single character.

- **Line-by-line scrolling (`Ctrl+e` / `Ctrl+y`)** — scroll the viewport one line down or up without moving the cursor (unless it would leave the visible area). Works in Normal and Visual modes.

- **`ToggleComment` / `ToggleBlockComment` editor actions** — `gcc` in Normal mode returns `EditorAction::ToggleComment`; `gc` in Visual mode returns `EditorAction::ToggleBlockComment { start_row, end_row }`. The consumer (e.g. dbtui) implements the actual commenting logic.

### Fixed

- **`yy` on a single line then `p` pasted inline instead of as a new line** — linewise yanks now append a trailing `\n` when copying to the system clipboard, so `resolve_paste_register()` correctly detects single-line yanks as linewise.
- **`Ctrl+e` scroll did nothing / `Ctrl+y` had no effect** — `ensure_cursor_visible()` had a hard `max_offset` clamp that reset the scroll after every keystroke; removed so the viewport can scroll freely past the last screenful (showing `~` tildes). Scroll methods now push the cursor respecting `SCROLLOFF` so `ensure_cursor_visible` doesn't undo the scroll.

## [0.1.7] - 2026-04-06

### Fixed

- **`p`/`P` in visual mode now replaces selection** — pressing `p` or `P` while in visual mode (char, line, or block) now deletes the selection and pastes the register/clipboard content in its place, matching standard Vim behavior. Previously, `p` was silently ignored in visual mode.
- **`x` in normal mode now copies to system clipboard** — deleted characters are now written to the system clipboard (via `wl-copy`/`xclip`/`xsel`), so they can be pasted in other applications or with `p`. Previously, `x` only saved to the internal unnamed register.
- **`x` with count accumulated correctly** — `3x` now puts all three deleted characters into the register. Previously, each iteration of the loop overwrote the register, keeping only the last character.

## [0.1.6] - 2026-04-05

### Added

- **Gutter diff signs (opt-in)** — new `GutterConfig` struct and `GutterSign` enum for visual diff indicators in the gutter. Set `editor.gutter = Some(GutterConfig { signs, .. })` to enable. Shows added (green `│`), modified (yellow `│`), deleted-above (red `▲`), and deleted-below (red `▼`) markers. Line numbers change color for added/modified lines. **Fully backward compatible** — `VimTheme` and `VimEditor::new()` are unchanged; when `gutter` is `None` (the default), rendering is identical to 0.1.5.

  ```rust
  use vimltui::{GutterConfig, GutterSign};

  editor.gutter = Some(GutterConfig {
      signs: my_computed_signs,  // HashMap<usize, GutterSign>
      ..Default::default()       // colors: Green, Yellow, Red
  });
  ```

### Fixed

- **`p`/`P` now read from system clipboard** — previously, paste only used the internal `unnamed_register`, ignoring the system clipboard. This broke paste between different editor instances within the same app and from external programs. Now `p`/`P` always try `wl-paste`/`xclip`/`xsel` first, falling back to the internal register only if no clipboard tool is available.
- **Linewise detection for system clipboard paste** — content from the system clipboard that ends with `\n` is now correctly detected as linewise, so pasting a yanked line inserts it on a new line (like Vim) instead of inline.
- **Multi-line clipboard paste collapsed into one line** — copying a multi-line block from another editor instance or external program and pasting with `p`/`P` now correctly inserts each line separately. Previously, content with internal newlines but not ending in `\n` was treated as characterwise and dumped into a single line.
- **`Ctrl+V` paste in search (`/`) and command (`:`) modes** — system clipboard content can now be pasted into the search and command input buffers with `Ctrl+V`. Only the first line is used. Previously, `Ctrl+V` was silently ignored in these modes.
- **Cursor shape now changes per mode** — the renderer now applies the correct terminal cursor shape: block for Normal/Visual, bar for Insert, underline for Replace (`R`) and single-char replace (`r`). Previously `cursor_shape()` returned the right value but the renderer never applied it.

### Code quality

- Extracted `read_system_clipboard() -> Option<String>` as the inverse of the existing `copy_to_system_clipboard`, using the same tool priority order (Wayland → X11 xclip → X11 xsel).
- Removed dead `use_system_clipboard` flag assignments — the flag was set but never read since yank always writes to the system clipboard and paste now always reads from it.

## [0.1.5] - 2026-04-04

### Fixed

- **`cargo install` broken for consumers** — the permissive `ratatui = ">=0.26, <1.0"` range caused cargo to resolve two incompatible ratatui versions (0.26 + 0.30) when consumers pinned an older version, producing `ratatui::style::Color` vs `ratatui_core::style::color::Color` type mismatches. Pinned to `ratatui = "0.30"`, `crossterm = "0.29"`, `unicode-width = "0.2"`.
- **Deprecated `frame.set_cursor()`** — replaced with `frame.set_cursor_position()` (ratatui 0.30 API).

### Code quality

- Suppressed `clippy::too_many_arguments` and `clippy::type_complexity` warnings surfaced by newer clippy.

## [0.1.4] - 2026-04-04

### Added

- **Delete key** — works in all modes: Insert/Replace (forward delete), Normal (same as `x`), Visual (same as `d`), and as operator motion (e.g., `d<Delete>` maps to line start).
- **Home/End keys** — works in all modes: maps to line start (`0`) and line end (`$`). Also works as operator motions (e.g., `d<Home>`, `y<End>`).
- **Arrow keys in Insert/Replace mode** — Left, Right, Up, Down move the cursor without leaving insert mode.
- **Visual mode count prefix** — number + motion now works in Visual mode (e.g., `v10j` selects 10 lines down, `v5w` selects 5 words). Also added missing motions: `W`/`B`/`E` (big-word), `^` (first non-blank), `%` (bracket match), `Ctrl+F`/`Ctrl+B` (full page scroll).

### Fixed

- **Ctrl+Char inserting characters in Insert/Replace mode** — unhandled Ctrl+key combinations (e.g., Ctrl+H from Ctrl+Delete) no longer insert the raw character; they are now silently ignored. Previously, only Ctrl+S/W/U were guarded, so any other Ctrl+letter fell through to the character-insert branch.

### Code quality

- Collapsed nested `else { if }` blocks into `else if` in bracket matching (motions.rs).
- Replaced manual `starts_with` + slice with `strip_prefix` in substitution parsing.
- Added `Delete` key replay support in dot-repeat (`.`).

## [0.1.3] - 2026-04-03

### Added

- **Live substitution preview** — when typing `:s` or `:%s`, matches highlight in real-time AND the replacement is shown live in the editor (like Neovim's `inccommand`). New `preview_lines` and `preview_highlights` fields on `VimEditor` for custom renderers. Replacement text is visually distinguished with `substitute_preview_bg` theme color.
- **Smartcase** — search (`/`, `?`, `*`, `#`) and substitution (`:s`) are now case-insensitive when the pattern is all-lowercase, and case-sensitive when it contains any uppercase character. The `i` flag in `:s` still forces case-insensitive.
- **Replace mode (`R`)** — overwrites characters instead of inserting. Shows `-- REPLACE --` in command line. At end of line, acts as insert. Exit with Esc.
- **`CursorShape` API** — new `cursor_shape()` method on `VimEditor` returns `Block` (Normal/Visual), `Bar` (Insert), or `Underline` (Replace/pending `r`). Custom renderers can use this to set terminal cursor style.
- **Visual mode case operations** — `u` (lowercase), `U` (UPPERCASE), `~` (toggle case) on the visual selection. Works with Char, Line, and Block selections.
- **`g~` operator** — toggle case with motion in normal mode (e.g., `g~w`, `g~$`). Complements existing `gu`/`gU`.
- **`r` with count** — `5rx` replaces 5 characters with `x`.

### Fixed

- **Preview highlights persisting after `:s` confirm** — `preview_highlights` now clears on Enter and Backspace-exit, not just Esc.

## [0.1.2] - 2026-04-03

### Added

- **Live substitution pattern highlight** — when typing a `:s` or `:%s` command, the search pattern is highlighted in real-time in the editor. Highlights clear on Esc, Enter, or Backspace past the pattern.

## [0.1.1] - 2026-04-03

### Fixed

- **`a` (append)** — cursor now correctly moves right when at the last character of a line; previously it stayed in place because `move_right()` was called while still in Normal mode, clamping to `line_len - 1`
- **`p`/`P` (paste)** — now correctly pastes on a new line when the register is linewise (after `yy`, `dd`), and inline when characterwise (after `yw`, `dw`); previously always pasted inline via system clipboard

### Added

- **Autoindent on Enter** — pressing Enter in insert mode now copies the leading whitespace from the current line, matching Vim's `autoindent` behavior
- **Yank highlight** — 150ms flash on yanked text after `yy` or visual yank (like Neovim's `vim.highlight.on_yank()`); new `yank_highlight_bg` color in `VimTheme`
- **Normal mode shortcuts** — `D` (delete to EOL), `C` (change to EOL), `Y` (yank line), `X` (delete before cursor), `S` (substitute line), `J` (join lines)
- **Bracket matching** — `%` jumps to the matching `()`, `{}`, `[]`; also works as operator motion (`d%`, `y%`)
- **Repeat find** — `;` repeats and `,` reverses the last `f`/`F`/`t`/`T` find
- **Word search** — `*` searches forward and `#` searches backward for the word under cursor
- **Scroll commands** — `zz` (center), `zt` (top), `zb` (bottom) screen positioning; `Ctrl-f`/`Ctrl-b` full page scroll; `H`/`M`/`L` jump to screen top/middle/bottom
- **Insert mode editing** — `Ctrl-w` deletes word backward, `Ctrl-u` deletes to start of line
- **Visual mode** — `o` swaps cursor and anchor, `c` changes (deletes selection and enters insert mode)
- **Text objects** — `a`-prefix (around): `aw`, `a"`, `a'`, `` a` ``, `a(`, `a{`, `a[`, `a<`; additional `i`-prefix: `i{`, `i[`, `i<`, `` i` ``, `ib`, `iB`
- **Substitution commands** — `:s/pat/rep/[flags]`, `:%s/pat/rep/[flags]`, `:N,Ms/pat/rep/[flags]` with full regex support (Rust `regex` crate), `g` (global) and `i` (case-insensitive) flags, custom delimiters, escaped delimiters
- **`:noh` / `:nohlsearch`** — clear search highlights

### Dependencies

- Added `regex = "1"` for substitution command support

## [0.1.0] - 2026-04-03

### Added

- `VimEditor` — self-contained editor that owns its text, cursor, mode, and state
- **Normal mode** — motions (`h/j/k/l`, `w/b/e`, `W/B/E`, `0/^/$`, `gg/G`), operators (`d/c/y`), count prefix (`3dw`), undo/redo (`u/Ctrl+R`)
- **Insert mode** — `i/I/a/A/o/O` entry points, auto-indentation on `o/O`, dot repeat (`.`)
- **Visual mode** — Char (`v`), Line (`V`), Block (`Ctrl+V`) with delete/yank/indent operations
- **Operator + Motion** composition — `dw`, `ci"`, `y$`, `>j`, `gUw`, `dd`, `yy`, `cc`, `>>`, `<<`
- **f/F/t/T** character find motions (with operator support: `df,`, `ct)`)
- **Search** — `/` forward, `?` backward, `n/N` navigation, case-insensitive, wrapping
- **Search highlighting** — all matches highlighted, current match distinguished with accent color
- **Command mode** — `:w`, `:q`, `:q!`, `:wq`, `:x`, `:123` (goto line)
- **Text objects** — `iw`, `i"`, `i'`, `i(`, `i{`
- **Replace** (`r`) and **substitute** (`s`) commands
- **Case operations** — `~` toggle, `gu`/`gU` operators
- **Indent/Dedent** — `>>`, `<<`, visual `>`/`<`
- **Registers** and system clipboard integration (wl-copy, xclip, xsel)
- **Built-in renderer** — relative line numbers, visual selection, search highlights, cursor, command line
- `SyntaxHighlighter` trait — plug in language-specific coloring (SQL, JSON, YAML, etc.)
- `PlainHighlighter` — no-op default highlighter
- `VimTheme` — customizable editor colors
- `VimModeConfig` — restrict available modes (e.g., read-only viewers)
- `EditorAction` — generic return type for parent application integration

[0.1.5]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.5
[0.1.4]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.4
[0.1.3]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.3
[0.1.2]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.2
[0.1.1]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.1
[0.1.0]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.0
