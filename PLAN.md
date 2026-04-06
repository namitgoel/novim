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

## v0.3.0 — Complete

### Clean Code & Bug Fixes
- Fixed 39 clippy warnings → 0
- Fixed UTF-8 panic risk in TUI file finder (byte-slicing → char-based)
- Fixed byte/char mismatch in GUI renderer padding
- Simplified redundant conditions (GUI Cmd shortcut logic)
- Added `#[derive(Debug, Clone)]` on `EditorCommand`

### Welcome Screen
- ASCII logo + shortcut hints (e/t/f/?/q)
- TUI-only — persistent until user picks an action
- GUI opens terminal directly (iTerm/Kitty behavior)

### TUI Startup Behavior
- `novim` → welcome screen
- `novim <file>` → editor
- `novim <dir>` → explorer sidebar + empty panel
- Terminal panes open only via commands

### GUI Improvements
- Syntax highlighting / text coloring working
- Popup overlay preserves syntax colors behind it
- `:q` respawns terminal instead of closing GUI window
- No status bar in pure terminal mode
- Removed `is_pure_terminal` fast path that broke popups
- Renderer split: `render_terminal_mode()` + `render_editor_mode()`
- Extracted shared `apply_popup_overlays()` + `flatten_screen_lines()`

### Ctrl+W Fix
- Added `gui_mode: bool` param to `key_to_command()`
- GUI: Ctrl+W in terminal forwards to PTY (delete word)
- TUI: Ctrl+W still works as pane command prefix

### Vim Motions
- `w` (word forward), `b` (word backward), `e` (end of word)
- `0` (line start), `$` (line end)
- `gg` (file start), `G` (file end)
- Works with operators: `dw`, `d$`, `cw`, `cb`
- Works with counts: `3w`, `5b`, `2dw`
- Works in visual mode to extend selection

### Text Objects
- `diw`, `ciw` — inner/change word
- `di"`, `ci"`, `da"` — inner/around quotes (", ', `)
- `di(`, `da{`, `ci[`, `da<` — inner/around brackets
- Full operator + i/a + object input state machine

### System Clipboard
- `arboard` crate for cross-platform clipboard
- Yank/delete/dd automatically copy to system clipboard
- Paste reads from system clipboard first
- `Cmd+C` (copy), `Cmd+V` (paste) in GUI

### GUI Tab Shortcuts
- `Cmd+1` through `Cmd+9` jump directly to tabs

### Configurable Settings
- `scroll_lines` (default 10) and `mouse_scroll_lines` (default 3) in config
- `gui.font_family` and `gui.font_size` configurable via TOML
- Extended `ThemeConfig` with 25+ color fields (foreground, background, cursor, selection, search, diagnostics, git signs, tabs, popups)
- `GuiTheme` struct ready for progressive color migration

### Git Gutter Signs
- `git2` crate integration
- `diff_signs()` computes +/~/- per line from HEAD
- Buffer tracks `git_signs: HashMap<usize, GitSign>`
- `PaneDisplay::git_sign(line)` trait method for renderers

### URL Detection
- `find_urls()`, `url_at_position()`, `open_url()` in `novim-core/src/url.rs`
- Cross-platform open (macOS: `open`, Linux: `xdg-open`, Windows: `cmd`)

### Auto-Reload
- Buffer tracks `last_modified: Option<SystemTime>`
- `EditorState::check_external_changes()` polls open files
- Clean buffers auto-reload; dirty buffers skip
- `Buffer::reload_from_file()` re-reads from disk

### Debug Logging
- `--debug` flag writes to `~/.novim/debug.log`
- GUI writes to `~/.novim/gui-debug.log`
- Filters wgpu/naga/winit noise

---

## v1.0.0 — Complete

### Plugin System Architecture

```
crates/novim-core/src/plugin/
├── mod.rs           # Plugin trait, PluginAction, BufferSnapshot, EditorEvent,
│                    #   PluginContext, KeymapRegistry, GutterSign, PluginError
├── lua_bridge.rs    # LuaPlugin — wraps Lua scripts as Plugin impls
│                    #   Full novim.* API, snapshot injection, action queue
├── manager.rs       # PluginManager — lifecycle, dispatch, timer polling,
│                    #   Lua plugin discovery, keymap registry
├── registry.rs      # CommandRegistry — plugin-defined ex-commands
└── builtins/
    ├── mod.rs       # register_builtins()
    ├── git_signs.rs # Git gutter signs (owns git2 diff logic, fully plugin-driven)
    └── syntax.rs    # Tree-sitter syntax highlighting (moved from core)
```

**Plugin trait:**
```rust
pub trait Plugin: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn init(&mut self, ctx: &mut PluginContext);
    fn shutdown(&mut self) {}
    fn on_event(&mut self, event: &EditorEvent, ctx: &PluginContext) -> Vec<PluginAction>;
    fn is_builtin(&self) -> bool { false }
    fn poll_timers(&mut self) -> Vec<PluginAction> { vec![] }
}
```

**Architecture:**
- Lua never holds `&mut EditorState` — reads via `BufferSnapshot`, writes via `PluginAction` queue
- `PluginManager` on `EditorState`, dispatched from `execute()` after each command
- Unknown `:` commands route through `CommandRegistry` → `PluginCommand` → plugin dispatch
- Plugin keymaps checked before config keybindings in TUI + GUI event loops
- `poll_timers()` called every 16ms tick for scheduled/deferred callbacks
- Plugins loaded from `~/.config/novim/init.lua` + `~/.config/novim/plugins/*.lua`
- Built-in features disabled via config: `[plugins.git_signs] enabled = false`

### Lua Plugin API

| Category | Functions |
|----------|-----------|
| **Core** | `novim.on(event, [opts], fn)`, `novim.exec(cmd)`, `novim.register_command(name, fn)` |
| **Buffer Read** | `novim.buf.get_lines(start, end)`, `get_text()`, `line_count()`, `path()`, `is_dirty()`, `cursor()` |
| **Buffer Write** | `novim.buf.set_lines(start, end, lines)`, `insert(line, col, text)`, `set_cursor(line, col)` |
| **Selection** | `novim.buf.get_selection()`, `selected_text()`, `set_selection(sl, sc, el, ec)`, `clear_selection()` |
| **Shell/File** | `novim.fn.shell(cmd)`, `readfile(path)`, `writefile(path, lines)`, `glob(pattern)` |
| **UI** | `novim.ui.status(msg)`, `log(msg)`, `popup(title, lines, [opts])` |
| **Keymaps** | `novim.keymap(mode, key, cmd_or_fn)` |
| **Options** | `novim.opt.get(name)`, `novim.opt.set(name, value)` |
| **Windows** | `novim.win.split(dir)`, `close()`, `count()` |
| **Autocmd Filter** | `novim.on("BufWrite", { pattern = "*.rs" }, fn)` |
| **Events** | `novim.emit(name, data)` — custom plugin-to-plugin events |
| **Scheduling** | `novim.schedule(fn)`, `novim.defer(ms, fn)` |
| **LSP** | `novim.lsp.on_attach(fn)` |

**Popup system:**
- `novim.ui.popup(title, lines)` — display-only, scrollable
- `novim.ui.popup(title, lines, { width=60, height=20 })` — custom size
- `novim.ui.popup(title, lines, { on_select=fn(index, text) })` — selectable list with highlighted cursor, Enter to select

**Editor Events:** BufOpen, BufEnter, BufLeave, BufWrite, BufClose, TextChanged, CursorMoved, ModeChanged, CommandExecuted, Custom, LspAttach

**PluginAction variants:** ExecCommand, SetLines, InsertText, SetCursor, SetStatus, RegisterKeymap, SetSelection, ClearSelection, EmitEvent, SetGutterSigns, ShowPopup

### Built-in Feature Migration

| Feature | Status | Location |
|---------|--------|----------|
| Git signs | Fully migrated | `plugin/builtins/git_signs.rs` — owns `git2` diff logic |
| Syntax highlighting | Module moved | `plugin/builtins/syntax.rs` — still Buffer-integrated |
| LSP | Not migrated | Too stateful/async for plugin pattern |
| Explorer | Not migrated | Needs dedicated UI panel |
| Finder | Not migrated | Needs popup UI with fuzzy matching |

### Commands Added

| Command | Description |
|---------|-------------|
| `:PluginList` / `:plugins` | Show all loaded plugins with status |
| `:echo <msg>` | Display message in status bar |
| `:<PluginCmd>` | Route to plugin-registered commands |

### Example Plugins

```
examples/plugins/
├── auto_save.lua        # Save on leaving insert mode
├── bookmark.lua         # Line bookmarks with selectable popup (Ctrl+b / :Bookmarks)
├── format_on_save.lua   # rustfmt/black/prettier on save (pattern-filtered)
├── git_branch.lua       # Show git branch on file open
├── quick_run.lua        # :Run — execute file with appropriate interpreter
├── trim_whitespace.lua  # Strip trailing whitespace on save
├── word_count.lua       # Ctrl+g — file stats popup
└── zen_mode.lua         # :Zen — toggle distraction-free mode
```

### Multi-Platform Release

```
              novim-core (engine + plugin system)
                       │
        ┌──────────────┼──────────────┐
        │              │              │
  novim-cli       novim-gui      novim-neon
  (terminal)      (desktop)      (Node.js)
    Ratatui       wgpu/winit      Neon FFI
```

### Test Coverage

27 plugin tests covering: Plugin lifecycle, event dispatch, command registry, Lua event handlers, command registration, buffer read/write API, shell/file API, UI status, options get/set, window API, autocmd filtering, selection API, custom events, scheduling, defer, LSP on_attach.

---

## v1.1.0 — Complete

### Vim Features Added
- **Named registers** — `"ayy` yank into register `a`, `"ap` paste from `a`, `"+` system clipboard
- **Marks** — `ma` set mark, `'a` line jump, `` `a `` exact position jump
- **Jump list** — `Ctrl+O` back, `Ctrl+I` forward, auto-push before gd/:e/search/gg/G
- **Dot repeat** — `.` replays last edit (dd, cw+text, diw, etc.) with insert text capture
- **Block visual** — `Ctrl+V` enters V-BLOCK column selection mode
- **`:substitute` flags** — `:%s/foo/bar/i` for case-insensitive replace
- **Ex-command ranges** — `5,10d` parses line ranges
- **`:source`** — `:source path` hot-reloads a Lua plugin (unregisters old commands/keymaps)
- **`yy`/`yw`/`y$`** — Yank operator (y + motion, yy for whole line)

### GUI Parity
- Plugin popup rendering (selectable lists with j/k/Enter)
- Plugin timer polling (`novim.defer()` / `novim.schedule()` work in GUI)

### Editor Polish
- Git branch displayed in TUI status bar
- `:set ts?` / `:set all` — query current option values
- Plugin load errors surfaced in status bar
- Error auto-disable — plugins disabled after 5 consecutive errors

### Keybindings Added

| Key | Mode | Action |
|-----|------|--------|
| `Ctrl+O` | Normal | Jump back |
| `Ctrl+V` | Normal | Block visual mode |
| `ma` | Normal | Set mark `a` |
| `'a` / `` `a `` | Normal | Jump to mark (line / exact) |
| `.` | Normal | Repeat last edit |
| `"a` | Normal | Select register `a` for next d/y/p |
| `yy` | Normal | Yank current line |
| `yw` / `y$` / etc. | Normal | Yank with motion |

---

## v1.2.0 — Complete

### Code Quality Refactoring
- Split `execute_inner()` (819 lines) into 14 focused handler methods
- Split `setup_api_and_run()` (580 lines) into 8 API setup methods
- Extracted shared `text_utils` module (`expand_tabs`, `display_col` with `Cow<str>`)
- Deduplicated LSP polling via `LspPollResult` struct
- Deduplicated jump navigation, buffer constructors, screen area helpers
- Removed unnecessary clones in hot paths (search patterns, syntax theme, dot repeat)
- Added error logging for silent failures (PTY reads, LSP spawn)
- Added input state machine safety guard
- Replaced hardcoded tab numbers with range pattern (GUI)
- Extracted `run_terminal()` helper in CLI

### File Splitting
- `editor.rs` (2354 lines) → `editor/` module (mod.rs, types.rs, workspace.rs, handlers.rs, input.rs)
- TUI `renderer.rs` (1741 lines) → `renderer/` module (mod.rs, pane.rs, popups.rs, styling.rs, util.rs)
- GUI `renderer.rs` (1755 lines) → `renderer/` module (mod.rs, theme.rs, pane.rs, popups.rs, styling.rs)
- `lua_bridge.rs` (1614 lines) → `lua_bridge/` module (mod.rs, api.rs, dispatch.rs, tests.rs)

### Bug Fixes
- Fixed multi-byte char panics in syntax highlighting (tree-sitter byte offsets → char boundaries)
- Fixed multi-byte char panics in search highlights, selection highlights, diagnostics, file finder preview, word wrap
- Fixed hover popup positioning (used buffer line instead of screen-relative position)
- Fixed `FindChar`/`TillChar` panic on empty lines (byte slicing → char iteration)
- Fixed render-loop panics in GUI (expect → graceful error logging)
- Added panic hook to restore terminal on crash

### Vim Features Added
- **Character find** — `f`/`F`/`t`/`T` + `;`/`,` repeat
- **Single-char ops** — `x` (delete forward), `r` (replace), `~` (toggle case)
- **Line operations** — `o`/`O` (open line), `J` (join), `A`/`I` (append/insert at bounds), `C`/`D`/`S` (change/delete to end, substitute)
- **Indentation** — `>>`/`<<` with count
- **Search** — `*`/`#` (word under cursor), `%` (matching bracket)
- **Paste before** — `P`
- **Replace mode** — `R` (overtype mode)
- **Visual reselect** — `gv`
- **Display line motion** — `gj`/`gk`
- **Paragraph/sentence motion** — `{`/`}`/`(`/`)`
- **Scroll** — `zz`/`zt`/`zb` (center/top/bottom), `Ctrl+B`/`PageUp`/`PageDown` (full page)
- **Confirm substitute** — `:%s/foo/bar/c` with y/n/a/q interactive prompt
- **Open URL/file** — `gx` (URL under cursor), `gf` (file under cursor)
- **List commands** — `:marks`, `:registers`/`:reg`
- **Shell execution** — `:!cmd`
- **Command history** — Up/Down in `:` mode

### tmux Features Added
- **Pane resize** — `Ctrl+W +/-/>/<`
- **Pane zoom** — `Ctrl+W z` (toggle full-screen pane)
- **Pane swap** — `Ctrl+W x`
- **Terminal copy mode** — `Ctrl+W [` with j/k scrollback navigation
- **Scrollback buffer** — 10,000 lines stored in VecDeque

### Terminal Features Added
- **OSC 7** — Shell CWD integration (zsh/bash report working directory)

### Help Popup Updated
- Navigation section expanded: motions, find, paragraph, sentence, bracket, search, scroll
- Editing section expanded: all new insert/change/delete commands, registers, dot repeat
- Pane section expanded: resize, zoom, swap, copy mode
- Commands section: gx, gf, marks, registers, shell, jump list

---

## v2.0.0 — Complete

### Code Quality
- Deduplicated TUI/GUI shared code: moved `snap_to_char_boundary`, `char_col_to_byte`, `truncate_str`, `wrap_line`, `wrapped_row_count` to `novim-core/text_utils.rs`
- Extracted `HighlightGroup::theme_color()` and `is_bold()` to eliminate duplicated match arms
- Created shared `help.rs` module — single source of truth for help popup content (TUI + GUI)
- Extracted `StatusBarInfo` + `status_bar_info()` — shared status bar computation
- Data-driven command registry: `BUILTIN_COMMANDS` table drives both `parse_ex_command()` and tab completion

### Vim Features Added
- **Yank to EOL** — `Y` maps to `y$` (neovim behavior)
- **Scroll one line** — `Ctrl+E`/`Ctrl+Y` scroll viewport without moving cursor
- **Screen jump** — `H`/`M`/`L` jump to top/middle/bottom of visible screen
- **Jump to last position** — `''`/` `` ` jumps before last jump (uses jump list)
- **Change directory** — `:cd`/`:lcd` with `~` expansion
- **File info** — `Ctrl+G` shows filename, line count, cursor percentage
- **Visual indent** — `>`/`<` in visual mode indent/dedent selection
- **Visual case** — `~`/`U`/`u` in visual mode toggle/upper/lower case
- **Increment/decrement** — `Ctrl+A`/`Ctrl+X` find number at/after cursor, +1/-1
- **Case with motion** — `gU`/`gu` + motion, `gUU`/`guu` for whole line
- **Insert file/cmd** — `:read file` / `:read !cmd` insert content below cursor
- **Sort lines** — `:sort` sorts all buffer lines
- **Pipe to command** — `:w !cmd` sends buffer to external command stdin
- **Auto-indent** — `==` re-indents current line to match previous line
- **Tab completion** — `Tab`/`Shift+Tab` in `:` mode completes commands, file paths, `:set` options
- **Quickfix list** — `:copen`, `:cn`/`:cp`, `:cclose`, `:make` with compiler output parsing
- **Command window** — `q:` opens scrollable command history, Enter to execute
- **Format text** — `gq` + motion / `gqq` wraps text to `text_width` (default 80)

### tmux Features Added
- **Copy mode selection** — `v` starts selection, `y` yanks text, `h/j/k/l` cursor movement in scrollback
- **Status line customization** — configurable `[status_bar]` with `left`/`right` format templates and placeholders (`{mode}`, `{file}`, `{line}`, `{lsp}`, `{branch}`, etc.)

### WezTerm / Terminal Features Added
- **24-bit true color** — `CellColor::Rgb(u8, u8, u8)`, SGR `38;2;R;G;B` and `48;2;R;G;B` parsing
- **OSC 133 prompt markers** — shell integration, prompt positions stored for navigation
- **OSC 8 clickable hyperlinks** — URL stored per cell, mouse click opens URL via `open_url()`

---

## v2.1.0 — Complete

### Plugin System
- **Plugin manifest** — `plugin.toml` with name, version, description, author, dependencies, entry point
- **Plugin install** — `:PlugInstall <url>` clones from git, `:PlugUpdate`, `:PlugRemove`
- **Floating windows** — `novim.ui.float(title, lines, opts)` plugin API, TUI rendering with scroll, Esc to close
- Example plugin: `float_preview.lua` (`:Preview`, `:Changelog`, `Ctrl+h` cheatsheet)

### Code Navigation
- **Tree-sitter symbols** — `Ctrl+T` / `:symbols` opens fuzzy-filterable symbol list popup
- Symbol extraction for Rust (functions, structs, enums, traits, modules, consts), JS/TS (functions, methods, classes), Python (functions, classes)
- `SymbolInfo` with `name`, `kind`, `line`, `end_line`, `depth` for nesting
- **Symbol outline sidebar** — `:outline` toggles persistent sidebar with indented symbol tree
- Icons per type: `ƒ` function, `◆` struct, `◇` enum, `◈` trait, `▸` module, `●` const
- Color-coded by kind, auto-highlights current symbol based on cursor position
- **Breadcrumb bar** — shows current location as `module > struct > function` above pane area
- Updates on every cursor movement via `breadcrumb_at()` containment check

### Git Integration
- **Inline blame** — `:blame`/`:Gblame` toggles per-line blame (author, date, commit summary)
- Uses `git2` crate for blame computation, displayed as dim italic text after line content
- **Diff view** — `:diff`/`:Gdiff` opens vertical split with HEAD version
- Syntax-highlighted HEAD buffer with proper display label (`file.rs (HEAD)`)
- Line-level diff highlighting: green (added), red (removed), yellow (changed)
- Diff highlights auto-clear when closing the diff pane

### Code Overview
- **Minimap** — `:minimap` / `:set minimap` toggles Braille-character code overview
- Each terminal cell = 2×4 dot grid for sub-character resolution
- Viewport region highlighted in blue, cursor line in yellow
- Click-to-jump: mouse click on minimap scrolls to that line
- Configurable: `minimap_width` (default 8), off by default

### Keybindings Added

| Key | Mode | Action |
|-----|------|--------|
| `Y` | Normal | Yank to EOL |
| `Ctrl+E`/`Ctrl+Y` | Normal | Scroll one line |
| `H`/`M`/`L` | Normal | Screen top/middle/bottom |
| `''` | Normal | Jump to last position |
| `Ctrl+G` | Normal | File info |
| `Ctrl+A`/`Ctrl+X` | Normal | Increment/decrement number |
| `gU`/`gu` + motion | Normal | Uppercase/lowercase with motion |
| `gUU`/`guu` | Normal | Uppercase/lowercase line |
| `gq` + motion / `gqq` | Normal | Format/wrap text |
| `==` | Normal | Auto-indent line |
| `>`/`<` | Visual | Indent/dedent selection |
| `~`/`U`/`u` | Visual | Case operations on selection |
| `v` | Copy mode | Start/toggle selection |
| `y` | Copy mode | Yank selected text |
| `Tab`/`Shift+Tab` | Command | Complete command/path/option |
| `q:` | Normal | Command history window |
| `Ctrl+T` | Normal | Symbol list popup |

---

## v2.2.0 — Polish

### Bug Fixes
- **`:q` should check all dirty buffers** — currently only checks the focused pane; users can lose unsaved work in other panes. Should warn about ALL dirty buffers across all panes before quitting.

### UX Improvements
- **`:set all` show all options** — currently only shows 5 options (ts, et, ai, wrap, ln). Should also display: minimap, text_width, scroll_lines, mouse_scroll_lines, finder_preview.
- **Feedback for invalid commands** — `Noop` commands are silently ignored. Invalid key sequences and unrecognized ex-commands should show a brief status message ("Unknown command: foo" or "Invalid key sequence").
- **Async git branch detection** — `git branch --show-current` runs synchronously at startup, blocking the editor. Should run on a background thread via `TaskRunner` and populate the status bar when ready.

---

## v3.0.0 — Future

### Near-term
- AI code completion (ghost text from LLM APIs, Tab to accept)
- Full Vim compatibility layer

### Medium-term
- DAP (Debug Adapter Protocol) — breakpoints, stepping, variable inspection

### Long-term
- Collaborative editing (CRDT-based)
- Remote development (SSH + local GUI)
- Web version (WASM + WebGPU)
