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
use novim_core::input::{key_to_command, key_to_string, lookup_custom_keybinding, EditorCommand, InputState};

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

    pub fn with_welcome() -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::new_welcome() })
    }

    pub fn with_file(path: &str) -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::with_file(path)? })
    }

    pub fn with_dir(path: &str) -> io::Result<Self> {
        Ok(Self { terminal: init_terminal()?, state: EditorState::with_dir(path)? })
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

    fn screen_area(&self) -> io::Result<novim_types::Rect> {
        let size = self.terminal.size()?;
        Ok(novim_types::Rect::new(0, 0, size.width, size.height))
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
        // Compute folds for the initial buffer
        let tw = self.state.config.editor.tab_width;
        self.state.focused_buf_mut().recompute_folds(tw);

        // Note: BufOpen is now emitted from EditorState constructor and handle_edit_file

        loop {
            // Poll terminals for all workspaces, LSP only for inactive ones
            for ws in self.state.tabs.iter_mut() {
                ws.poll_terminals();
            }

            // Poll background tasks (git blame, :make, etc.)
            self.state.poll_tasks();

            // Check for external file changes (auto-reload)
            self.state.check_external_changes();

            // Poll plugin scheduled/deferred callbacks
            let timer_actions = self.state.plugins.poll_timers();
            if !timer_actions.is_empty() {
                let screen_area = self.screen_area()?;
                self.state.run_plugin_actions(timer_actions, screen_area);
            }

            // Reparse syntax highlights only for the active workspace
            self.state.focused_buf_mut().reparse_highlights();

            let state = &mut self.state;
            self.terminal.draw(|frame| renderer::render(frame, state))?;

            if event::poll(std::time::Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) => {
                        // Welcome screen: only shortcut keys work, others ignored
                        if self.state.show_welcome {
                            let cmd = match key.code {
                                KeyCode::Char('e') => Some(EditorCommand::SwitchMode(EditorMode::Insert)),
                                KeyCode::Char('t') => Some(EditorCommand::OpenTerminal),
                                KeyCode::Char('f') => Some(EditorCommand::OpenFileFinder),
                                KeyCode::Char('?') => Some(EditorCommand::ToggleHelp),
                                KeyCode::Char('q') => Some(EditorCommand::Quit),
                                _ => { continue; } // ignore non-shortcut keys
                            };
                            if let Some(cmd) = cmd {
                                self.state.show_welcome = false;
                                let screen_area = self.screen_area()?;
                                if self.exec(cmd, screen_area) {
                                    break;
                                }
                            }
                            continue;
                        }

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
                            let screen_area = self.screen_area()?;
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
                                let screen_area = self.screen_area()?;
                                self.exec(cmd, screen_area);
                                continue;
                            }
                        }

                        // Confirm replace mode: intercept y/n/a/q
                        if self.state.confirm_replace.active {
                            let cmd = match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => EditorCommand::ReplaceConfirmYes,
                                KeyCode::Char('n') | KeyCode::Char('N') => EditorCommand::ReplaceConfirmNo,
                                KeyCode::Char('a') | KeyCode::Char('A') => EditorCommand::ReplaceConfirmAll,
                                KeyCode::Char('q') | KeyCode::Esc => EditorCommand::ReplaceConfirmQuit,
                                _ => EditorCommand::Noop,
                            };
                            if !matches!(cmd, EditorCommand::Noop) {
                                let screen_area = self.screen_area()?;
                                self.exec(cmd, screen_area);
                            }
                            continue;
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

                        // Plugin popup: j/k to move selection, Enter to select, Esc/q to dismiss
                        if self.state.plugin_popup.is_some() {
                            match key.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if let Some(popup) = &mut self.state.plugin_popup {
                                        if popup.selected + 1 < popup.lines.len() {
                                            popup.selected += 1;
                                        }
                                        // Auto-scroll to keep selection visible
                                        let visible_h = popup.height.unwrap_or(popup.lines.len() as u16 + 2).saturating_sub(2) as usize;
                                        if popup.selected >= popup.scroll + visible_h {
                                            popup.scroll = popup.selected.saturating_sub(visible_h - 1);
                                        }
                                    }
                                    continue;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if let Some(popup) = &mut self.state.plugin_popup {
                                        popup.selected = popup.selected.saturating_sub(1);
                                        if popup.selected < popup.scroll {
                                            popup.scroll = popup.selected;
                                        }
                                    }
                                    continue;
                                }
                                KeyCode::Enter => {
                                    let screen_area = self.screen_area()?;
                                    self.state.handle_popup_select(screen_area);
                                    continue;
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    self.state.plugin_popup = None;
                                    continue;
                                }
                                _ => { continue; }
                            }
                        }

                        // Symbol list: filter, navigate, accept
                        if self.state.symbol_list.visible {
                            let cmd = match key.code {
                                KeyCode::Esc => EditorCommand::SymbolDismiss,
                                KeyCode::Enter => EditorCommand::SymbolAccept,
                                KeyCode::Up | KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => EditorCommand::SymbolUp,
                                KeyCode::Down | KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => EditorCommand::SymbolDown,
                                KeyCode::Up => EditorCommand::SymbolUp,
                                KeyCode::Down => EditorCommand::SymbolDown,
                                KeyCode::Backspace => EditorCommand::SymbolBackspace,
                                KeyCode::Char(c) => EditorCommand::SymbolInput(c),
                                _ => EditorCommand::Noop,
                            };
                            if !matches!(cmd, EditorCommand::Noop) {
                                let screen_area = self.screen_area()?;
                                self.exec(cmd, screen_area);
                            }
                            continue;
                        }

                        // Floating window: Esc closes topmost
                        if !self.state.floating_windows.is_empty() {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    let screen_area = self.screen_area()?;
                                    self.exec(EditorCommand::CloseFloat, screen_area);
                                    continue;
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    if let Some(fw) = self.state.floating_windows.last_mut() {
                                        let max = fw.lines.len().saturating_sub(1);
                                        fw.scroll = (fw.scroll + 1).min(max);
                                    }
                                    continue;
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    if let Some(fw) = self.state.floating_windows.last_mut() {
                                        fw.scroll = fw.scroll.saturating_sub(1);
                                    }
                                    continue;
                                }
                                _ => { continue; }
                            }
                        }

                        // Command window: j/k navigate, Enter executes, q/Esc closes
                        if self.state.command_window.visible {
                            match key.code {
                                KeyCode::Char('k') | KeyCode::Up => {
                                    if self.state.command_window.selected > 0 {
                                        self.state.command_window.selected -= 1;
                                    }
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    if self.state.command_window.selected + 1 < self.state.command_history.len() {
                                        self.state.command_window.selected += 1;
                                    }
                                }
                                KeyCode::Enter => {
                                    let idx = self.state.command_window.selected;
                                    if let Some(cmd_str) = self.state.command_history.get(idx).cloned() {
                                        self.state.command_window.visible = false;
                                        let parsed = novim_core::input::parse_ex_command(&cmd_str);
                                        let screen_area = self.screen_area()?;
                                        self.exec(parsed, screen_area);
                                    }
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    self.state.command_window.visible = false;
                                }
                                _ => {}
                            }
                            continue;
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
                            let screen_area = self.screen_area()?;
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
                            let screen_area = self.screen_area()?;
                            self.exec(cmd, screen_area);
                            continue;
                        }

                        let in_terminal = self.state.focused_buf().is_terminal();

                        // Copy mode: intercept keys before terminal forwarding
                        if in_terminal {
                            let focused_id = self.state.tabs[self.state.active_tab].panes.focused_id();
                            let copy_offset = self.state.tabs[self.state.active_tab].panes
                                .get_pane(focused_id).map(|p| p.copy_mode_offset).unwrap_or(0);
                            if copy_offset > 0 {
                                let cmd = novim_core::editor::handle_copy_mode_key(
                                    &mut self.state.tabs[self.state.active_tab].panes,
                                    focused_id,
                                    key,
                                    &mut self.state.registers,
                                    &mut self.state.status_message,
                                );
                                if !matches!(cmd, EditorCommand::Noop) {
                                    let screen_area = self.screen_area()?;
                                    self.exec(cmd, screen_area);
                                }
                                continue;
                            }
                        }

                        // Any key dismisses hover
                        if self.state.hover_text.is_some() {
                            self.state.hover_text = None;
                        }
                        let popup_showing = self.state.show_help || self.state.tabs[self.state.active_tab].show_buffer_list || self.state.show_workspace_list || self.state.plugin_popup.is_some();

                        // Check plugin keymaps first (before borrowing config)
                        let key_str = key_to_string(&key);
                        let mode_str = self.state.mode.display_name();
                        if !key_str.is_empty() {
                            let screen_area = self.screen_area()?;
                            if self.state.try_plugin_keymap(mode_str, &key_str, screen_area) {
                                self.state.input_state = InputState::Ready;
                                continue;
                            }
                        }

                        // Check custom keybindings (config override)
                        let custom_bindings = match self.state.mode {
                            EditorMode::Normal => &self.state.config.keybindings.normal,
                            EditorMode::Insert => &self.state.config.keybindings.insert,
                            _ => &self.state.config.keybindings.normal,
                        };
                        let (cmd, new_input_state) = if let Some(custom_cmd) = lookup_custom_keybinding(&key, custom_bindings) {
                            (custom_cmd, InputState::Ready)
                        } else {
                            key_to_command(self.state.mode, self.state.input_state, key, in_terminal, popup_showing, false, self.state.macros.recording.is_some())
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
                                EditorCommand::ScrollUp | EditorCommand::ScrollDown => cmd,
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

                        let screen_area = self.screen_area()?;
                        if self.exec(cmd, screen_area) {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let screen_area = self.screen_area()?;
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

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore terminal before printing panic
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        default_hook(info);
    }));
}

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    setup_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}
