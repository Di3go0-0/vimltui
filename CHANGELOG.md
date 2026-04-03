# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.3]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.3
[0.1.2]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.2
[0.1.1]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.1
[0.1.0]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.0
