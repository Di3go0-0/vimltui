# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/Di3go0-0/vimltui/releases/tag/v0.1.0
