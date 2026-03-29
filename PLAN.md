# Novim Implementation Plan

## v0.1.0 — Complete

### What Was Built
- Modal editing (Normal/Insert/Visual/Command modes) with Vim keybindings
- Rope-based text buffer (Ropey) with O(log n) operations
- BSP pane tree (horizontal/vertical splits, focus switching)
- Terminal emulator (PTY + VTE with ANSI color parsing)
- Session persistence (save/restore layouts to JSON)
- Ex-command system (`:w`, `:q`, `:q!`, `:wq`, `:split`, `:vsplit`, `:terminal`, `:e`, `:mksession`, `:help`)
- Help popup (`?` or `Ctrl+W ?`)
- Command pattern for all key actions (EditorCommand enum)
- BufferLike trait for polymorphic pane content
- Unified error handling (NovimError)

---

## v0.2.0 — Complete

### Code Quality Refactors
- Extracted sub-states from EditorState: `SearchState`, `FinderState`, `CompletionState`, `MacroState`
- Deduplicated EditorState constructors via `with_config_and_tabs()`
- Deduplicated Workspace constructors via `new_with()`
- Split `BufferLike` into focused sub-traits: `PaneDisplay`, `TextEditing`, `Searchable`, `TerminalLike` with blanket `BufferLike` supertrait
- Added `focused_buf()` / `focused_buf_mut()` helpers to eliminate deep accessor chains
- Added `with_lsp_client()` helper to deduplicate LSP boilerplate (ShowHover, GotoDefinition, TriggerCompletion)
- Fixed duplicate LSP polling: inactive tabs use `Workspace::poll_lsp()`, active tab uses `EditorState::poll_active_lsp()`
- Changed `execute()` return type from `bool` to `Result<ExecOutcome, NovimError>` for structured errors
- Shared `LspRegistry` via `Arc` across all workspaces instead of cloning per-workspace
- Fixed `insert_char` missing `version += 1` (bug: stale LSP didChange versions)
- Added rope text cache (`cached_text: Option<String>`) to avoid repeated `rope.to_string()` in search/replace/reparse
- Deduplicated `Buffer::from_file()` constructor
- Eliminated all compiler warnings

### Features Added
- **Undo/Redo** — grouped edit operations, `u` / `Ctrl+R`
- **Visual Mode** — `v` to select, `d`/`y` to act, `Esc` to cancel
- **Clipboard** — `y` yank, `p` paste (editor-internal)
- **Search** — `/pattern` (regex), `n`/`N` next/prev, `:%s/old/new` replace all
- **Colored Terminal** — ANSI color rendering in terminal panes
- **Relative Line Numbers** — hybrid (cursor=absolute, others=relative), `:set rnu/nonu`
- **Mouse Support** — click to position cursor/focus pane, scroll viewport
- **Buffer Management** — `:bn`, `:bp`, `:ls` buffer list
- **File Explorer** — `:explore` sidebar, tree navigation
- **File Finder** — `Ctrl+F` fuzzy file search with preview
- **Syntax Highlighting** — Tree-sitter (Rust, JS/TS, Python, JSON, TOML, Markdown)
- **Configuration** — `~/.config/novim/config.toml` (theme, keybindings, tab_width, LSP)
- **LSP Integration** — autocomplete, go-to-definition (`gd`), hover (`K`), diagnostics
- **Macros** — `Qa` record, `@a` replay, `@@` repeat last
- **Count Prefixes** — `5j`, `3dd`, `2dw`
- **Operator-Pending** — `d`+motion, `c`+motion
- **Multi-Workspace** — `:tabnew`, `gt`/`gT`, workspace list
- **Tab/Indent Settings** — `expandtab`, `auto_indent`, `tab_width` config; Tab key inserts spaces; Enter preserves indentation; `:set et/noet/ai/noai/ts=N`
- **Word Wrap** — `:set wrap/nowrap`; wrap-aware viewport scrolling and cursor positioning
- **Multi-Cursor** — `Alt+Up/Down` to add cursors; simultaneous editing at all cursors; `Esc` clears
- **Code Folding** — indent-based fold detection; `za` toggle, `zM` fold all, `zR` unfold all; fold-aware cursor navigation

### Architecture

```
crates/
├── novim-types/     # Shared types (Position, Rect, EditorMode, Direction, Selection)
├── novim-core/      # Engine (rlib) — no rendering dependencies
│   ├── buffer/      # PaneDisplay + TextEditing + Searchable + TerminalLike traits, Ropey Buffer
│   ├── pane/        # BSP tree pane manager
│   ├── emulator/    # PTY + VTE + Grid (CellColor/CellAttrs)
│   ├── session/     # Save/restore sessions
│   ├── explorer.rs  # File explorer tree
│   ├── finder.rs    # Fuzzy file finder
│   ├── fold.rs      # Code folding (indent-based)
│   ├── highlight.rs # Tree-sitter syntax highlighting
│   ├── lsp/         # LSP client, provider registry, transport
│   ├── input.rs     # EditorCommand + key mapping + ex-command parser
│   ├── config.rs    # TOML configuration
│   └── error.rs     # NovimError
├── novim-tui/       # Terminal UI (rlib) — Ratatui renderer
│   ├── lib.rs       # EditorState (with sub-states) + TerminalManager + event loop
│   └── renderer.rs  # All rendering logic (wrap-aware, fold-aware, multi-cursor)
└── novim-neon/      # Node.js FFI (cdylib) — Neon bindings

packages/
└── novim/           # TypeScript CLI (Commander.js + Chalk)
    ├── src/core.ts  # NovimCore class wrapping native module
    └── src/index.ts # CLI entry (novim, novim <file>, novim attach, novim list)
```

### Keybindings

| Key | Mode | Action |
|-----|------|--------|
| `i` | Normal | Enter Insert mode |
| `v` | Normal | Enter Visual mode |
| `Esc` | Insert/Visual/Command | Return to Normal mode |
| `hjkl` / arrows | Normal | Navigate |
| `5j` / `3k` | Normal | Move N lines |
| `:` | Normal | Enter Command mode |
| `?` | Normal | Help popup |
| `q` | Normal | Quit |
| `u` / `Ctrl+R` | Normal | Undo / Redo |
| `p` | Normal | Paste |
| `dd` / `3dd` | Normal | Delete line(s) |
| `cc` | Normal | Change line |
| `dl` / `dh` | Normal | Delete char right/left |
| `/pattern` | Normal | Search (regex) |
| `n` / `N` | Normal | Next / prev match |
| `:%s/old/new` | Command | Replace all |
| `Ctrl+S` / `:w` | Normal | Save |
| `:q` / `:q!` / `:wq` | Command | Quit / Force / Save+quit |
| `:e <file>` | Command | Open file |
| `Ctrl+F` | Normal | File finder |
| `:explore` | Command | File explorer |
| `:ls` | Command | Buffer list |
| `:bn` / `:bp` | Command | Next / prev buffer |
| `gd` | Normal | Go to definition (LSP) |
| `K` | Normal | Hover info (LSP) |
| `Ctrl+N` | Insert | Autocomplete (LSP) |
| `za` | Normal | Toggle fold at cursor |
| `zM` | Normal | Fold all |
| `zR` | Normal | Unfold all |
| `Alt+Up` | Normal | Add cursor above |
| `Alt+Down` | Normal | Add cursor below |
| `Qa` / `@a` / `@@` | Normal | Record / replay / repeat macro |
| `Ctrl+W v/s` | Any | Vertical / horizontal split |
| `Ctrl+W h/j/k/l` | Any | Move focus between panes |
| `Ctrl+W q` | Any | Close pane |
| `Ctrl+W t` | Any | Open terminal |
| `Ctrl+W w` | Any | Cycle focus |
| `:tabnew <path>` | Command | New workspace |
| `gt` / `gT` | Normal | Next / prev workspace |
| `:set wrap/nowrap` | Command | Toggle word wrap |
| `:set et/noet` | Command | Expand tab on/off |
| `:set ai/noai` | Command | Auto-indent on/off |
| `:set ts=N` | Command | Set tab width |
| `:set rnu/nonu` | Command | Line number mode |

### CLI Commands

```bash
novim                    # Open shell (like tmux)
novim <file>             # Open file in editor
novim attach <name>      # Restore saved session
novim list               # List saved sessions
novim info               # Show project status
novim test               # Test native module
```

### Build Commands

```bash
cargo build --release
cp target/release/libnovim_neon.dylib packages/novim/native/novim_core.node
pnpm -w run build:ts
cargo test
node packages/novim/dist/index.js test
```

---

## v1.0.0 — Multi-Platform Release

Ship three distribution targets from the same `novim-core` engine:

### Phase 1: Standalone Terminal App (`novim-cli`)

**What it is**: The TUI app packaged as a single binary (no Node.js dependency).

**Changes needed**:
- Create `crates/novim-cli/` — a Rust binary crate that calls `novim-tui` directly
- CLI arg parsing via `clap`: `novim`, `novim <file>`, `novim --session <name>`
- Single binary: `cargo build --release -p novim-cli` → `target/release/novim`
- Distribution: Homebrew, `cargo install`, GitHub releases

```
crates/novim-cli/
  ├── Cargo.toml    # depends on novim-tui, clap
  └── src/main.rs   # clap args → TerminalManager::with_file/new/with_session
```

**Result**: `novim` works in any terminal — Terminal.app, iTerm2, Kitty, Alacritty, WezTerm, SSH sessions.

### Phase 2: VS Code-Style GUI App (`novim-gui`)

**What it is**: A native desktop editor with GPU-rendered UI, native shortcuts, font rendering.

**Tech stack**:
```
crates/novim-gui/
  ├── Cargo.toml     # wgpu, winit, cosmic-text
  ├── src/
  │   ├── main.rs    # Window creation, event loop
  │   ├── renderer.rs # GPU text rendering
  │   ├── theme.rs    # Visual theme (colors, fonts, spacing)
  │   └── input.rs    # OS key events → EditorCommand
```

**Dependencies**:
- `winit` — cross-platform window management
- `wgpu` — GPU rendering (WebGPU API, macOS/Windows/Linux)
- `cosmic-text` — text shaping, font loading, ligatures

**What it gains over TUI**:
- Native Cmd+S, Cmd+C, Cmd+V
- Any font with ligatures (Fira Code, JetBrains Mono)
- Smooth pixel scrolling
- Inline image previews, markdown rendering
- Floating panels, detachable panes
- Native file dialogs

**What it reuses from novim-core** (everything):
- Buffer + undo/redo + multi-cursor
- Pane BSP tree
- Terminal emulator (PTY)
- Command pattern (EditorCommand)
- Session persistence
- Search/replace
- Syntax highlighting (Tree-sitter)
- LSP client
- Code folding
- Configuration

### Phase 3: Embeddable Terminal Widget (`novim-neon`)

**What it is**: The existing Node.js native module — Novim running inside other apps.

**Use cases**:
- Electron apps embedding a terminal+editor
- VS Code extension providing Novim-style editing
- Web-based IDEs via wasm (future)

### Architecture Diagram

```
                    novim-core (engine)
                    ├── buffer + undo + multi-cursor
                    ├── pane tree
                    ├── emulator (PTY)
                    ├── session
                    ├── input/commands
                    ├── syntax (tree-sitter)
                    ├── code folding
                    ├── config
                    └── LSP client
                         │
          ┌──────────────┼──────────────┐
          │              │              │
    novim-cli       novim-gui      novim-neon
    (terminal)      (desktop)      (Node.js)
      │                │              │
    Ratatui         wgpu/winit      Neon FFI
      │                │              │
    Any terminal    Native window   Electron/etc.
```

### Build Targets

```bash
# Terminal app (single binary, no Node.js)
cargo build --release -p novim-cli
./target/release/novim

# GUI app (native desktop)
cargo build --release -p novim-gui
./target/release/novim-gui

# Node.js module (for embedding)
cargo build --release -p novim-neon
```

---

## v2.0.0+ — Long-term Vision

- Collaborative editing (CRDT-based)
- Remote development (SSH + local GUI)
- Web version (WASM + WebGPU)
- AI code completion integration
- Plugin system (TypeScript-based, leveraging Neon FFI)
- Vim emulation layer for full Vim compatibility
- Git integration (inline blame, diff view)
- Minimap / code overview
- Breadcrumbs / symbol outline
