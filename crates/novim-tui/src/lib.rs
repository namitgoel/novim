//! Novim TUI — Ratatui-based terminal user interface.
//!
//! This crate provides the terminal rendering and crossterm event loop.
//! All editor state and logic lives in novim-core::editor.

pub mod renderer;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use novim_types::EditorMode;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

use novim_core::editor::ExecOutcome;
use novim_core::input::{key_to_command, lookup_custom_keybinding, EditorCommand, InputState};

// Re-export editor types so renderer and downstream crates can use them.
pub use novim_core::editor::{
    CompletionState, EditorState, FinderState, LineNumberMode, MacroState, SearchState, Workspace,
};

/// Manages terminal lifecycle — init, event loop, shutdown.
pub struct TerminalManager {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    state: EditorState,
}

impl TerminalManager {
    pub fn new() -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::new_editor() })
    }

    pub fn with_file(path: &str) -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::with_file(path)? })
    }

    pub fn with_session(name: &str) -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::from_session(name)? })
    }

    pub fn with_terminal() -> io::Result<Self> {
        let terminal = init_terminal()?;
        let size = terminal.size()?;
        Ok(Self {
            terminal,
            state: EditorState::new_terminal(size.height.saturating_sub(1), size.width)?,
        })
    }

    /// Execute a command, displaying errors as status messages. Returns true if should quit.
    fn exec(&mut self, cmd: EditorCommand, screen_area: novim_types::Rect) -> bool {
        match self.state.execute(cmd, screen_area) {
            Ok(ExecOutcome::Quit) => true,
            Ok(ExecOutcome::Continue) => false,
            Err(e) => {
                self.state.status_message = Some(e.to_string());
                false
            }
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        // Start LSP for the initial buffer
        if self.state.config.lsp.enabled {
            self.state.tabs[self.state.active_tab].ensure_lsp_for_buffer(true);
        }

        // Compute folds for the initial buffer
        let tw = self.state.config.editor.tab_width;
        self.state.focused_buf_mut().recompute_folds(tw);

        loop {
            // Poll terminals for all workspaces, LSP only for inactive ones
            let active = self.state.active_tab;
            for (i, ws) in self.state.tabs.iter_mut().enumerate() {
                ws.poll_terminals();
                if i != active {
                    ws.poll_lsp();
                }
            }

            // Process LSP events for the active workspace (updates EditorState fields)
            self.state.poll_active_lsp();

            // Reparse syntax highlights only for the active workspace
            self.state.focused_buf_mut().reparse_highlights();

            let state = &mut self.state;
            self.terminal.draw(|frame| renderer::render(frame, state))?;

            if event::poll(std::time::Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) => {
                        // File finder active: route keys to finder
                        if self.state.finder.visible {
                            let cmd = match key.code {
                                KeyCode::Esc => EditorCommand::FinderDismiss,
                                KeyCode::Enter => EditorCommand::FinderAccept,
                                KeyCode::Up => EditorCommand::FinderUp,
                                KeyCode::Down => EditorCommand::FinderDown,
                                KeyCode::Backspace => EditorCommand::FinderBackspace,
                                KeyCode::Char(c) => EditorCommand::FinderInput(c),
                                _ => EditorCommand::Noop,
                            };
                            let size = self.terminal.size()?;
                            let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                            self.exec(cmd, screen_area);
                            continue;
                        }

                        // Completion menu active: route keys to completion
                        if self.state.completion.visible {
                            let cmd = match key.code {
                                KeyCode::Up | KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => EditorCommand::CompletionUp,
                                KeyCode::Down | KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => EditorCommand::CompletionDown,
                                KeyCode::Up => EditorCommand::CompletionUp,
                                KeyCode::Down => EditorCommand::CompletionDown,
                                KeyCode::Tab | KeyCode::Enter => EditorCommand::CompletionAccept,
                                KeyCode::Esc => EditorCommand::CompletionDismiss,
                                _ => {
                                    self.state.completion.visible = false;
                                    self.state.completion.items.clear();
                                    EditorCommand::Noop
                                }
                            };
                            if !matches!(cmd, EditorCommand::Noop) {
                                let size = self.terminal.size()?;
                                let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                                self.exec(cmd, screen_area);
                                continue;
                            }
                        }

                        // Help popup: scroll with arrows
                        if self.state.show_help {
                            match key.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    self.state.help_scroll += 1;
                                    continue;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    self.state.help_scroll = self.state.help_scroll.saturating_sub(1);
                                    continue;
                                }
                                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                                    self.state.show_help = false;
                                    self.state.help_scroll = 0;
                                    continue;
                                }
                                _ => {
                                    continue;
                                }
                            }
                        }

                        // Workspace list active: route keys to workspace selector
                        if self.state.show_workspace_list {
                            let handled = match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if self.state.workspace_list_selected > 0 {
                                        self.state.workspace_list_selected -= 1;
                                    }
                                    true
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if self.state.workspace_list_selected + 1 < self.state.tabs.len() {
                                        self.state.workspace_list_selected += 1;
                                    }
                                    true
                                }
                                KeyCode::Enter => {
                                    self.state.active_tab = self.state.workspace_list_selected;
                                    self.state.show_workspace_list = false;
                                    true
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    self.state.show_workspace_list = false;
                                    true
                                }
                                _ => {
                                    self.state.show_workspace_list = false;
                                    false
                                }
                            };
                            if handled {
                                continue;
                            }
                        }

                        // Explorer focused: route keys to explorer
                        if self.state.tabs[self.state.active_tab].explorer_focused && self.state.tabs[self.state.active_tab].explorer.is_some() {
                            let cmd = match key.code {
                                KeyCode::Char('j') | KeyCode::Down => EditorCommand::ExplorerDown,
                                KeyCode::Char('k') | KeyCode::Up => EditorCommand::ExplorerUp,
                                KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => EditorCommand::ExplorerOpen,
                                KeyCode::Esc | KeyCode::Char('q') => EditorCommand::ToggleExplorer,
                                KeyCode::Tab => {
                                    self.state.tabs[self.state.active_tab].explorer_focused = false;
                                    EditorCommand::Noop
                                }
                                _ => EditorCommand::Noop,
                            };
                            let size = self.terminal.size()?;
                            let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                            self.exec(cmd, screen_area);
                            continue;
                        }

                        // Search mode intercepts keys before normal dispatch
                        if self.state.search.active {
                            let cmd = match key.code {
                                KeyCode::Esc => EditorCommand::SearchCancel,
                                KeyCode::Enter => EditorCommand::SearchExecute,
                                KeyCode::Backspace => EditorCommand::SearchBackspace,
                                KeyCode::Char(c) => EditorCommand::SearchInput(c),
                                _ => EditorCommand::Noop,
                            };
                            let size = self.terminal.size()?;
                            let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                            self.exec(cmd, screen_area);
                            continue;
                        }

                        let in_terminal = self.state.focused_buf().is_terminal();
                        // Any key dismisses hover
                        if self.state.hover_text.is_some() {
                            self.state.hover_text = None;
                        }
                        let popup_showing = self.state.show_help || self.state.tabs[self.state.active_tab].show_buffer_list || self.state.show_workspace_list;

                        // Check custom keybindings first (config override)
                        let custom_bindings = match self.state.mode {
                            EditorMode::Normal => &self.state.config.keybindings.normal,
                            EditorMode::Insert => &self.state.config.keybindings.insert,
                            _ => &self.state.config.keybindings.normal,
                        };
                        let (cmd, new_input_state) = if let Some(custom_cmd) = lookup_custom_keybinding(self.state.mode, &key, custom_bindings) {
                            (custom_cmd, InputState::Ready)
                        } else {
                            key_to_command(self.state.mode, self.state.input_state, key, in_terminal, popup_showing)
                        };
                        // Handle count accumulation
                        if new_input_state == InputState::AccumulatingCount {
                            if let KeyCode::Char(c) = key.code {
                                if c.is_ascii_digit() {
                                    self.state.count_state.pending_digits.push(c);
                                    self.state.input_state = InputState::AccumulatingCount;
                                    continue;
                                }
                            }
                        }

                        // Apply accumulated count to movement commands
                        let cmd = if !self.state.count_state.pending_digits.is_empty() {
                            let count: usize = self.state.count_state.pending_digits.parse().unwrap_or(1);
                            self.state.count_state.pending_digits.clear();
                            match cmd {
                                EditorCommand::MoveCursor(dir) => EditorCommand::MoveCursorN(dir, count),
                                EditorCommand::DeleteMotion(dir, _) => EditorCommand::DeleteMotion(dir, count),
                                EditorCommand::ChangeMotion(dir, _) => EditorCommand::ChangeMotion(dir, count),
                                EditorCommand::DeleteLines(_) => EditorCommand::DeleteLines(count),
                                EditorCommand::ChangeLines(_) => EditorCommand::ChangeLines(count),
                                EditorCommand::ScrollUp => {
                                    for _ in 0..count { /* handled below */ }
                                    cmd
                                }
                                other => other,
                            }
                        } else {
                            cmd
                        };

                        self.state.input_state = new_input_state;

                        if matches!(cmd, EditorCommand::ForceRedraw) {
                            execute!(
                                self.terminal.backend_mut(),
                                LeaveAlternateScreen,
                                EnterAlternateScreen
                            )?;
                            continue;
                        }

                        // Record keystroke if macro recording is active
                        if self.state.macros.recording.is_some()
                            && !matches!(cmd, EditorCommand::StartMacroRecord(_) | EditorCommand::StopMacroRecord | EditorCommand::ReplayMacro(_))
                        {
                            self.state.macros.buffer.push(key);
                        }

                        let size = self.terminal.size()?;
                        let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                        if self.exec(cmd, screen_area) {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let size = self.terminal.size()?;
                        let screen_area = novim_types::Rect::new(0, 0, size.width, size.height);
                        self.state.handle_mouse(mouse, screen_area);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}
