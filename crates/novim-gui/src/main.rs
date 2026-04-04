//! Novim GUI — GPU-accelerated native desktop window.
//!
//! Uses wgpu for rendering, winit for windowing, and glyphon (cosmic-text)
//! for high-quality text shaping and rendering. Drives the same EditorState
//! as the TUI frontend.

mod gpu;
mod input;
mod renderer;

use crossterm::event::KeyCode;
use novim_core::editor::{EditorState, ExecOutcome};
use novim_core::input::{
    key_to_command, key_to_string, lookup_custom_keybinding, EditorCommand, InputState,
};
use novim_types::EditorMode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event::{MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::Window;

use gpu::GpuState;

/// Custom event sent from PTY reader threads to wake the event loop.
#[derive(Debug, Clone)]
enum UserEvent {
    PtyDataReady,
}

fn main() {
    init_gui_logger();
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("Failed to create event loop");

    // Register the PTY waker *before* any terminals are created.
    let proxy = event_loop.create_proxy();
    novim_core::emulator::set_pty_waker(Arc::new(move || {
        let _ = proxy.send_event(UserEvent::PtyDataReady);
    }));

    event_loop
        .run_app(&mut Application {
            window_state: None,
            file_arg: std::env::args().nth(1),
            last_tick: Instant::now(),
            needs_redraw: true,
            poll_dirty: Arc::new(AtomicBool::new(false)),
        })
        .expect("Event loop error");
}

/// Per-window state: GPU + editor.
pub(crate) struct WindowState {
    gpu: GpuState,
    editor: EditorState,
    /// Current mouse position in grid coordinates.
    mouse_col: u16,
    mouse_row: u16,
    /// Current keyboard modifier state.
    modifiers: winit::keyboard::ModifiersState,
    /// winit window handle (must outlive wgpu surface).
    window: Arc<Window>,
    /// Hash of the last rendered frame content — skip reshaping if unchanged.
    last_frame_hash: u64,
    /// Persistent text buffer reused across frames (avoids re-allocation).
    cached_text_buffer: glyphon::Buffer,
    /// Throttle window title updates (shell_cwd syscall) to ~1/sec.
    last_title_update: Instant,
}

use std::time::{Duration, Instant};

struct Application {
    window_state: Option<WindowState>,
    file_arg: Option<String>,
    last_tick: Instant,
    /// Whether a redraw is needed (set by key/mouse events, cleared after render).
    needs_redraw: bool,
    /// Shared flag set by the background poller when terminals/LSP produce output.
    poll_dirty: Arc<AtomicBool>,
}

impl winit::application::ApplicationHandler<UserEvent> for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window_state.is_some() {
            return;
        }

        let (width, height) = (1200, 800);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("Novim");
        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        let cfg = novim_core::config::load_config();
        let mut gpu = pollster::block_on(GpuState::new(
            window.clone(),
            &cfg.gui.font_family,
            cfg.gui.font_size,
        ));

        let mut editor = if let Some(ref path) = self.file_arg {
            let p = std::path::Path::new(path);
            if p.is_dir() {
                EditorState::with_dir(path).unwrap_or_else(|e| {
                    let mut ed = EditorState::new_editor();
                    ed.status_message = Some(format!("Failed to open dir {}: {}", path, e));
                    ed
                })
            } else {
                match EditorState::with_file(path) {
                    Ok(e) => e,
                    Err(e) => {
                        let mut ed = EditorState::new_editor();
                        ed.status_message = Some(format!("Failed to open {}: {}", path, e));
                        ed
                    }
                }
            }
        } else {
            // GUI acts as a terminal emulator — open terminal directly
            let rows = gpu.grid_rows();
            let cols = gpu.grid_cols();
            EditorState::new_terminal(rows, cols)
                .unwrap_or_else(|_| EditorState::new_editor())
        };
        // Compute initial folds for opened file.
        let tw = editor.config.editor.tab_width;
        editor.focused_buf_mut().recompute_folds(tw);

        // Note: BufOpen is emitted from EditorState constructor and handle_edit_file

        let cached_text_buffer = glyphon::Buffer::new(
            &mut gpu.font_system,
            glyphon::Metrics::new(gpu.font_size, gpu.line_height),
        );

        self.window_state = Some(WindowState {
            gpu,
            editor,
            mouse_col: 0,
            mouse_row: 0,
            modifiers: winit::keyboard::ModifiersState::empty(),
            window,
            last_frame_hash: 0,
            cached_text_buffer,
            last_title_update: Instant::now(),
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.window_state else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                let scale = state.window.scale_factor();
                state.gpu.resize(size.width, size.height, scale);
                // Resize all terminal panes to match the new window dimensions.
                let rows = state.gpu.grid_rows();
                let cols = state.gpu.grid_cols();
                for ws in &mut state.editor.tabs {
                    ws.resize_terminals(rows, cols);
                }
                state.window.request_redraw();
            }

            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let Some((key_event, is_super)) =
                    input::translate_key(&event.logical_key, event.state, state.modifiers)
                {
                    let screen = screen_rect(state);
                    if handle_key(&mut state.editor, key_event, screen, is_super) {
                        // Instead of closing the GUI, open a fresh terminal
                        let rows = state.gpu.grid_rows();
                        let cols = state.gpu.grid_cols();
                        state.editor = EditorState::new_terminal(rows, cols)
                            .unwrap_or_else(|_| EditorState::new_editor());
                    }
                    self.needs_redraw = true;
                    state.window.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.mouse_col =
                    (position.x as f32 / state.gpu.cell_width).floor() as u16;
                state.mouse_row =
                    (position.y as f32 / state.gpu.cell_height).floor() as u16;
            }

            WindowEvent::MouseInput { state: btn_state, button, .. } => {
                if let Some(mouse_event) = input::translate_mouse_button(
                    button,
                    btn_state,
                    state.mouse_col,
                    state.mouse_row,
                    state.modifiers,
                ) {
                    let screen = screen_rect(state);
                    state.editor.handle_mouse(mouse_event, screen);
                    self.needs_redraw = true;
                    state.window.request_redraw();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => {
                        (pos.y as f32) / state.gpu.cell_height
                    }
                };
                if let Some(mouse_event) =
                    input::translate_scroll(lines, state.mouse_col, state.mouse_row)
                {
                    let screen = screen_rect(state);
                    state.editor.handle_mouse(mouse_event, screen);
                    self.needs_redraw = true;
                    state.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                renderer::render(state);

                // Update window title at most once per second (shell_cwd is a syscall).
                let now = Instant::now();
                if now.duration_since(state.last_title_update) >= Duration::from_secs(1) {
                    state.last_title_update = now;
                    let buf = state.editor.focused_buf();
                    let title = if buf.is_terminal() {
                        let pane = state.editor.tabs[state.editor.active_tab].panes.focused_pane();
                        if let Some(cwd) = pane.content.as_buffer_like().shell_cwd() {
                            let dir = cwd.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| cwd.to_string_lossy().to_string());
                            format!("{} — Novim", dir)
                        } else {
                            "Novim".to_string()
                        }
                    } else {
                        let name = buf.display_name();
                        format!("{} — Novim", name)
                    };
                    state.window.set_title(&title);
                }
            }

            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        // PTY data arrived — drain it immediately and request redraw.
        self.needs_redraw = true;
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &mut self.window_state {
            // Always drain pending PTY data (non-blocking try_recv).
            let mut pty_dirty = false;
            for ws in state.editor.tabs.iter_mut() {
                if ws.poll_terminals() {
                    pty_dirty = true;
                }
            }

            // Poll plugins on a slower cadence (~50ms) since it's less latency-sensitive.
            let now = Instant::now();
            if now.duration_since(self.last_tick) >= Duration::from_millis(50) {
                self.last_tick = now;

                // Poll plugin scheduled/deferred callbacks (includes LSP)
                let screen = novim_types::Rect::new(0, 0, state.gpu.surface_config.width as u16, state.gpu.surface_config.height as u16);
                let timer_actions = state.editor.plugins.poll_timers();
                if !timer_actions.is_empty() {
                    state.editor.run_plugin_actions(timer_actions, screen);
                    self.needs_redraw = true;
                }
            }

            let poll_dirty = self.poll_dirty.swap(false, Ordering::Relaxed);
            if self.needs_redraw || poll_dirty || pty_dirty {
                state.window.request_redraw();
                self.needs_redraw = false;
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn screen_rect(state: &WindowState) -> novim_types::Rect {
    novim_types::Rect::new(0, 0, state.gpu.grid_cols(), state.gpu.grid_rows())
}

/// Process a keyboard event through the editor, mirroring the TUI's dispatch.
/// `is_super` is true when the macOS Cmd key was held — these shortcuts always
/// bypass terminal forwarding so they work as app-level commands.
/// Returns true if the editor wants to quit.
fn handle_key(
    editor: &mut EditorState,
    key: crossterm::event::KeyEvent,
    screen: novim_types::Rect,
    is_super: bool,
) -> bool {
    // ── Cmd (Super) shortcuts — always work, even in terminal panes ──
    // On macOS Cmd+key are "app-level" shortcuts, never forwarded to the PTY.
    if is_super {
        let cmd = match key.code {
            KeyCode::Char('p') | KeyCode::Char('f') => Some(EditorCommand::OpenFileFinder),
            KeyCode::Char('e') => Some(EditorCommand::ToggleExplorer),
            KeyCode::Char('b') => Some(EditorCommand::BufferList),
            KeyCode::Char('t') => Some(EditorCommand::OpenTerminal),
            KeyCode::Char('n') => Some(EditorCommand::NextTab),
            KeyCode::Char('w') => {
                editor.input_state = InputState::WaitingPaneCommand;
                editor.status_message = Some("Ctrl+W...".to_string());
                return false;
            }
            KeyCode::Char(':') => Some(EditorCommand::SwitchMode(EditorMode::Command)),
            KeyCode::Char('/') => Some(EditorCommand::EnterSearch),
            KeyCode::Char('?') => Some(EditorCommand::ToggleHelp),
            KeyCode::Char('c') => Some(EditorCommand::YankSelection),
            KeyCode::Char('v') => Some(EditorCommand::Paste),
            KeyCode::Char('s') => Some(EditorCommand::Save),
            KeyCode::Char('q') => Some(EditorCommand::Quit),
            // Cmd+1-9: jump to tab
            KeyCode::Char(c @ '1'..='9') => Some(EditorCommand::JumpToTab((c as usize) - ('1' as usize))),
            _ => None,
        };
        if let Some(cmd) = cmd {
            return exec(editor, cmd, screen);
        }
    }
    // Esc always resets any stuck intermediate input state.
    if key.code == KeyCode::Esc
        && editor.input_state != InputState::Ready
        && !editor.finder.visible
        && !editor.search.active
    {
        editor.input_state = InputState::Ready;
        editor.status_message = Some("Cancelled".to_string());
        return false;
    }

    // ── Finder overlay ──
    if editor.finder.visible {
        let cmd = match key.code {
            KeyCode::Esc => EditorCommand::FinderDismiss,
            KeyCode::Enter => EditorCommand::FinderAccept,
            KeyCode::Up => EditorCommand::FinderUp,
            KeyCode::Down => EditorCommand::FinderDown,
            KeyCode::Backspace => EditorCommand::FinderBackspace,
            KeyCode::Char(c) => EditorCommand::FinderInput(c),
            _ => EditorCommand::Noop,
        };
        return exec(editor, cmd, screen);
    }

    // ── Completion menu ──
    if editor.completion.visible {
        let cmd = match key.code {
            KeyCode::Up => EditorCommand::CompletionUp,
            KeyCode::Down => EditorCommand::CompletionDown,
            KeyCode::Tab | KeyCode::Enter => EditorCommand::CompletionAccept,
            KeyCode::Esc => EditorCommand::CompletionDismiss,
            _ => {
                editor.completion.visible = false;
                editor.completion.items.clear();
                EditorCommand::Noop
            }
        };
        if !matches!(cmd, EditorCommand::Noop) {
            return exec(editor, cmd, screen);
        }
    }

    // ── Confirm replace mode ──
    if editor.confirm_replace.active {
        let cmd = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => EditorCommand::ReplaceConfirmYes,
            KeyCode::Char('n') | KeyCode::Char('N') => EditorCommand::ReplaceConfirmNo,
            KeyCode::Char('a') | KeyCode::Char('A') => EditorCommand::ReplaceConfirmAll,
            KeyCode::Char('q') | KeyCode::Esc => EditorCommand::ReplaceConfirmQuit,
            _ => EditorCommand::Noop,
        };
        if !matches!(cmd, EditorCommand::Noop) {
            return exec(editor, cmd, screen);
        }
        return false;
    }

    // ── Help popup ──
    if editor.show_help {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                editor.help_scroll += 1;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                editor.help_scroll = editor.help_scroll.saturating_sub(1);
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                editor.show_help = false;
                editor.help_scroll = 0;
            }
            _ => {}
        }
        return false;
    }

    // ── Workspace list popup ──
    // Symbol list: filter, navigate, accept
    if editor.symbol_list.visible {
        let cmd = match key.code {
            KeyCode::Esc => EditorCommand::SymbolDismiss,
            KeyCode::Enter => EditorCommand::SymbolAccept,
            KeyCode::Up => EditorCommand::SymbolUp,
            KeyCode::Down => EditorCommand::SymbolDown,
            KeyCode::Backspace => EditorCommand::SymbolBackspace,
            KeyCode::Char(c) => EditorCommand::SymbolInput(c),
            _ => EditorCommand::Noop,
        };
        if !matches!(cmd, EditorCommand::Noop) {
            let _ = editor.execute(cmd, screen);
        }
        return false;
    }

    // Floating window: Esc closes topmost
    if !editor.floating_windows.is_empty() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let _ = editor.execute(EditorCommand::CloseFloat, screen);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(fw) = editor.floating_windows.last_mut() {
                    let max = fw.lines.len().saturating_sub(1);
                    fw.scroll = (fw.scroll + 1).min(max);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(fw) = editor.floating_windows.last_mut() {
                    fw.scroll = fw.scroll.saturating_sub(1);
                }
            }
            _ => {}
        }
        return false;
    }

    // Command window: j/k navigate, Enter executes, q/Esc closes
    if editor.command_window.visible {
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => {
                if editor.command_window.selected > 0 {
                    editor.command_window.selected -= 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if editor.command_window.selected + 1 < editor.command_history.len() {
                    editor.command_window.selected += 1;
                }
            }
            KeyCode::Enter => {
                let idx = editor.command_window.selected;
                if let Some(cmd_str) = editor.command_history.get(idx).cloned() {
                    editor.command_window.visible = false;
                    let parsed = novim_core::input::parse_ex_command(&cmd_str);
                    let _ = editor.execute(parsed, screen);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                editor.command_window.visible = false;
            }
            _ => {}
        }
        return false;
    }

    if editor.show_workspace_list {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if editor.workspace_list_selected > 0 {
                    editor.workspace_list_selected -= 1;
                }
                return false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if editor.workspace_list_selected + 1 < editor.tabs.len() {
                    editor.workspace_list_selected += 1;
                }
                return false;
            }
            KeyCode::Enter => {
                editor.active_tab = editor.workspace_list_selected;
                editor.show_workspace_list = false;
                return false;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                editor.show_workspace_list = false;
                return false;
            }
            _ => {
                editor.show_workspace_list = false;
            }
        }
    }

    // ── Explorer ──
    let ws = &editor.tabs[editor.active_tab];
    if ws.explorer_focused && ws.explorer.is_some() {
        let cmd = match key.code {
            KeyCode::Char('j') | KeyCode::Down => EditorCommand::ExplorerDown,
            KeyCode::Char('k') | KeyCode::Up => EditorCommand::ExplorerUp,
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => EditorCommand::ExplorerOpen,
            KeyCode::Esc | KeyCode::Char('q') => EditorCommand::ToggleExplorer,
            KeyCode::Tab => {
                // Can't borrow immutably above then mutably here, so drop the ref.
                editor.tabs[editor.active_tab].explorer_focused = false;
                EditorCommand::Noop
            }
            _ => EditorCommand::Noop,
        };
        return exec(editor, cmd, screen);
    }

    // ── Search mode ──
    if editor.search.active {
        let cmd = match key.code {
            KeyCode::Esc => EditorCommand::SearchCancel,
            KeyCode::Enter => EditorCommand::SearchExecute,
            KeyCode::Backspace => EditorCommand::SearchBackspace,
            KeyCode::Char(c) => EditorCommand::SearchInput(c),
            _ => EditorCommand::Noop,
        };
        return exec(editor, cmd, screen);
    }

    // ── Normal key dispatch ──
    let in_terminal = editor.focused_buf().is_terminal();

    // Copy mode: intercept keys before terminal forwarding
    if in_terminal {
        let focused_id = editor.tabs[editor.active_tab].panes.focused_id();
        let copy_offset = editor.tabs[editor.active_tab].panes
            .get_pane(focused_id).map(|p| p.copy_mode_offset).unwrap_or(0);
        if copy_offset > 0 {
            let cmd = novim_core::editor::handle_copy_mode_key(
                &mut editor.tabs[editor.active_tab].panes,
                focused_id,
                key,
                &mut editor.registers,
                &mut editor.status_message,
            );
            if !matches!(cmd, EditorCommand::Noop) {
                let _ = editor.execute(cmd, screen);
            }
            return false;
        }
    }

    if editor.hover_text.is_some() {
        editor.hover_text = None;
    }
    let popup_showing = editor.show_help
        || editor.tabs[editor.active_tab].show_buffer_list
        || editor.show_workspace_list
        || editor.plugin_popup.is_some();

    // Plugin popup: j/k to move, Enter to select, Esc/q to dismiss
    if editor.plugin_popup.is_some() {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(popup) = &mut editor.plugin_popup {
                    if popup.selected + 1 < popup.lines.len() {
                        popup.selected += 1;
                    }
                    let visible_h = popup.height.unwrap_or(popup.lines.len() as u16 + 2).saturating_sub(2) as usize;
                    if popup.selected >= popup.scroll + visible_h {
                        popup.scroll = popup.selected.saturating_sub(visible_h - 1);
                    }
                }
                return false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(popup) = &mut editor.plugin_popup {
                    popup.selected = popup.selected.saturating_sub(1);
                    if popup.selected < popup.scroll {
                        popup.scroll = popup.selected;
                    }
                }
                return false;
            }
            KeyCode::Enter => {
                editor.handle_popup_select(screen);
                return false;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                editor.plugin_popup = None;
                return false;
            }
            _ => return false,
        }
    }

    // Check plugin keymaps first (before borrowing config)
    let key_str = key_to_string(&key);
    let mode_str = editor.mode.display_name();
    if !key_str.is_empty() && editor.try_plugin_keymap(mode_str, &key_str, screen) {
        editor.input_state = InputState::Ready;
        return false;
    }

    let custom_bindings = match editor.mode {
        EditorMode::Normal => &editor.config.keybindings.normal,
        EditorMode::Insert => &editor.config.keybindings.insert,
        _ => &editor.config.keybindings.normal,
    };
    let (cmd, new_input_state) =
        if let Some(custom_cmd) = lookup_custom_keybinding(&key, custom_bindings) {
            (custom_cmd, InputState::Ready)
        } else {
            key_to_command(
                editor.mode,
                editor.input_state,
                key,
                in_terminal,
                popup_showing,
                true, // gui_mode: Ctrl+W forwards to PTY in terminal panes
                editor.macros.recording.is_some(),
            )
        };

    // Count accumulation
    if new_input_state == InputState::AccumulatingCount {
        if let KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() {
                editor.count_state.pending_digits.push(c);
                editor.input_state = InputState::AccumulatingCount;
                return false;
            }
        }
    }

    let cmd = if !editor.count_state.pending_digits.is_empty() {
        let count: usize = editor.count_state.pending_digits.parse().unwrap_or(1);
        editor.count_state.pending_digits.clear();
        match cmd {
            EditorCommand::MoveCursor(dir) => EditorCommand::MoveCursorN(dir, count),
            EditorCommand::DeleteMotion(dir, _) => EditorCommand::DeleteMotion(dir, count),
            EditorCommand::ChangeMotion(dir, _) => EditorCommand::ChangeMotion(dir, count),
            EditorCommand::DeleteLines(_) => EditorCommand::DeleteLines(count),
            EditorCommand::ChangeLines(_) => EditorCommand::ChangeLines(count),
            other => other,
        }
    } else {
        cmd
    };

    editor.input_state = new_input_state;

    // Macro recording
    if editor.macros.recording.is_some()
        && !matches!(
            cmd,
            EditorCommand::StartMacroRecord(_)
                | EditorCommand::StopMacroRecord
                | EditorCommand::ReplayMacro(_)
        )
    {
        editor.macros.buffer.push(key);
    }

    exec(editor, cmd, screen)
}

/// Execute a single command — returns true if the editor wants to quit.
fn exec(editor: &mut EditorState, cmd: EditorCommand, screen: novim_types::Rect) -> bool {
    match editor.execute(cmd, screen) {
        Ok(ExecOutcome::Quit) => true,
        Ok(ExecOutcome::Continue) => false,
        Err(e) => {
            editor.status_message = Some(e.to_string());
            false
        }
    }
}

fn init_gui_logger() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let log_dir = std::path::PathBuf::from(&home).join(".novim");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("gui-debug.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open GUI debug log file");
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .filter_module("wgpu", log::LevelFilter::Warn)
        .filter_module("naga", log::LevelFilter::Warn)
        .filter_module("winit", log::LevelFilter::Warn)
        .target(env_logger::Target::Pipe(Box::new(file)))
        .format_timestamp_millis()
        .init();
    log::info!("GUI debug logging enabled: {}", log_path.display());
}
