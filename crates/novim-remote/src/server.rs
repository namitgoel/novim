//! Remote server: runs on the remote machine, owns EditorState.
//!
//! Reads ClientMessage from stdin, executes commands, renders to TestBackend,
//! sends ServerMessage (cell grid) to stdout.

use std::io::{self, BufReader, BufWriter};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use novim_core::editor::EditorState;
use novim_core::input::key_to_command;
use novim_tui::renderer;

use crate::protocol::*;
use crate::transport;

/// Run the remote server. Reads from stdin, writes to stdout.
pub fn run_server(path: Option<&str>) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin_reader = BufReader::new(stdin.lock());
    let mut stdout = BufWriter::new(io::stdout().lock());

    // Read Hello message (before spawning reader thread)
    let hello: ClientMessage = transport::read_message(&mut stdin_reader)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "No Hello message"))?;

    let (width, height) = match hello {
        ClientMessage::Hello { version, width, height } => {
            if version != PROTOCOL_VERSION {
                let err = ServerMessage::Error {
                    message: format!("Protocol mismatch: server={}, client={}", PROTOCOL_VERSION, version),
                };
                transport::write_message(&mut stdout, &err)?;
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Protocol version mismatch"));
            }
            transport::write_message(&mut stdout, &ServerMessage::HelloAck { version: PROTOCOL_VERSION })?;
            (width, height)
        }
        _ => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Expected Hello message"));
        }
    };

    // Create editor state
    let mut state = match path {
        Some(p) => {
            let p_path = std::path::Path::new(p);
            if p_path.is_dir() {
                EditorState::with_dir(p)?
            } else {
                EditorState::with_file(p)?
            }
        }
        None => EditorState::new_welcome(),
    };

    // Create virtual terminal for rendering
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;

    let mut prev_cells: Vec<Vec<StyledCell>> = Vec::new();
    let mut screen_area = novim_types::Rect::new(0, 0, width, height);

    // Drop the locked stdin reader, spawn a new one in the reader thread
    drop(stdin_reader);

    // Spawn a reader thread for non-blocking stdin
    let (input_tx, input_rx) = mpsc::channel::<ClientMessage>();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match transport::read_message::<ClientMessage>(&mut reader) {
                Some(msg) => {
                    if input_tx.send(msg).is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
    });

    // Log to stderr (which SSH forwards to the client's terminal as errors)
    eprintln!("[novim-serve] Server started, {}x{}", width, height);

    let mut running = true;
    while running {
        // 1. Poll terminals, tasks, plugins
        for ws in state.tabs.iter_mut() {
            ws.poll_terminals();
        }
        state.poll_tasks();
        let timer_actions = state.plugins.poll_timers();
        if !timer_actions.is_empty() {
            state.run_plugin_actions(timer_actions, screen_area);
        }
        state.check_external_changes();

        // Reparse highlights
        if !state.focused_buf().is_terminal() {
            state.focused_buf_mut().reparse_highlights();
        }

        // 2. Process all pending input from the reader thread
        while let Ok(msg) = input_rx.try_recv() {
            match msg {
                ClientMessage::Key { code, modifiers } => {
                    let key_event = reconstruct_key(&code, modifiers);
                    if let Ok(novim_core::editor::ExecOutcome::Quit) = process_key_event(&mut state, key_event, screen_area) {
                        running = false;
                    }
                }
                ClientMessage::Mouse { kind, col, row, modifiers: _ } => {
                    process_mouse_event(&mut state, &kind, col, row, screen_area);
                }
                ClientMessage::Resize { width: w, height: h } => {
                    terminal.backend_mut().resize(w, h);
                    screen_area = novim_types::Rect::new(0, 0, w, h);
                    prev_cells.clear();
                    for ws in state.tabs.iter_mut() {
                        ws.resize_terminals(h.saturating_sub(2), w.saturating_sub(2));
                    }
                }
                ClientMessage::Ping => {
                    let _ = transport::write_message(&mut stdout, &ServerMessage::Pong);
                }
                ClientMessage::Disconnect => {
                    let _ = transport::write_message(&mut stdout, &ServerMessage::Bye);
                    running = false;
                }
                ClientMessage::Hello { .. } => {}
            }
        }

        // 3. Render to TestBackend
        terminal.draw(|f| renderer::render(f, &mut state))?;

        // 4. Extract cells and send frame if changed
        let buf = terminal.backend().buffer();
        let w = buf.area.width;
        let h = buf.area.height;
        let cells = extract_cells(terminal.backend(), w, h);
        let cursor_pos = None; // cursor is embedded in the rendered cells

        if cells != prev_cells {
            let cell_count: usize = cells.iter().map(|r| r.len()).sum();
            eprintln!("[novim-serve] Sending frame: {}x{} ({} cells)", w, h, cell_count);
            transport::write_message(&mut stdout, &ServerMessage::Frame {
                cells: cells.clone(),
                cursor: cursor_pos,
            })?;
            prev_cells = cells;
        }

        // 5. Small sleep to avoid busy-waiting
        thread::sleep(Duration::from_millis(16));
    }

    Ok(())
}

/// Extract styled cells from the TestBackend buffer.
fn extract_cells(backend: &TestBackend, width: u16, height: u16) -> Vec<Vec<StyledCell>> {
    let buf = backend.buffer();
    let mut cells = Vec::with_capacity(height as usize);

    for y in 0..height {
        let mut row = Vec::with_capacity(width as usize);
        for x in 0..width {
            let cell = &buf[(x, y)];
            let (fg_r, fg_g, fg_b) = color_to_rgb(cell.fg);
            let (bg_r, bg_g, bg_b) = color_to_rgb(cell.bg);

            let mut attrs = 0u8;
            if cell.modifier.contains(ratatui::style::Modifier::BOLD) { attrs |= 0b0001; }
            if cell.modifier.contains(ratatui::style::Modifier::DIM) { attrs |= 0b0010; }
            if cell.modifier.contains(ratatui::style::Modifier::UNDERLINED) { attrs |= 0b0100; }
            if cell.modifier.contains(ratatui::style::Modifier::REVERSED) { attrs |= 0b1000; }

            let c = cell.symbol().chars().next().unwrap_or(' ');
            row.push(StyledCell::new(c, [fg_r, fg_g, fg_b], [bg_r, bg_g, bg_b], attrs));
        }
        cells.push(row);
    }
    cells
}

/// Convert ratatui Color to RGB tuple.
fn color_to_rgb(color: ratatui::style::Color) -> (u8, u8, u8) {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        ratatui::style::Color::Black => (0, 0, 0),
        ratatui::style::Color::Red => (224, 108, 117),
        ratatui::style::Color::Green => (152, 195, 121),
        ratatui::style::Color::Yellow => (229, 192, 123),
        ratatui::style::Color::Blue => (97, 175, 239),
        ratatui::style::Color::Magenta => (198, 120, 221),
        ratatui::style::Color::Cyan => (86, 182, 194),
        ratatui::style::Color::White | ratatui::style::Color::Reset => (220, 220, 220),
        ratatui::style::Color::DarkGray => (100, 100, 100),
        ratatui::style::Color::LightRed => (240, 140, 140),
        ratatui::style::Color::LightGreen => (180, 220, 160),
        ratatui::style::Color::LightYellow => (240, 210, 150),
        ratatui::style::Color::LightBlue => (140, 200, 250),
        ratatui::style::Color::LightMagenta => (220, 160, 240),
        ratatui::style::Color::LightCyan => (130, 210, 220),
        ratatui::style::Color::Indexed(idx) => indexed_to_rgb(idx),
        ratatui::style::Color::Gray => (128, 128, 128),
    }
}

/// Approximate 256-color index to RGB.
fn indexed_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0..=7 => {
            let colors = [(0,0,0),(128,0,0),(0,128,0),(128,128,0),(0,0,128),(128,0,128),(0,128,128),(192,192,192)];
            colors[idx as usize]
        }
        8..=15 => {
            let colors = [(128,128,128),(255,0,0),(0,255,0),(255,255,0),(0,0,255),(255,0,255),(0,255,255),(255,255,255)];
            colors[(idx - 8) as usize]
        }
        16..=231 => {
            let idx = idx - 16;
            let r = (idx / 36) * 51;
            let g = ((idx % 36) / 6) * 51;
            let b = (idx % 6) * 51;
            (r, g, b)
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            (v, v, v)
        }
    }
}

/// Reconstruct a crossterm KeyEvent from serialized key string and modifier bitmask.
fn reconstruct_key(code: &str, mods: u8) -> KeyEvent {
    let modifiers = KeyModifiers::from_bits_truncate(mods);
    let key_code = match code {
        "Esc" => KeyCode::Esc,
        "Enter" => KeyCode::Enter,
        "Backspace" => KeyCode::Backspace,
        "Tab" => KeyCode::Tab,
        "BackTab" => KeyCode::BackTab,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        s if s.len() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        s if s.starts_with("F") => {
            if let Ok(n) = s[1..].parse::<u8>() {
                KeyCode::F(n)
            } else {
                KeyCode::Null
            }
        }
        _ => KeyCode::Null,
    };
    KeyEvent::new(key_code, modifiers)
}

/// Process a key event through the editor's dispatch logic. Returns ExecOutcome.
fn process_key_event(
    state: &mut EditorState,
    key: KeyEvent,
    screen_area: novim_types::Rect,
) -> Result<novim_core::editor::ExecOutcome, novim_core::error::NovimError> {
    let in_terminal = state.focused_buf().is_terminal();
    let popup_showing = state.show_help
        || state.tabs[state.active_tab].show_buffer_list
        || state.show_workspace_list
        || state.plugin_popup.is_some();

    let (cmd, new_input_state) = key_to_command(
        state.mode,
        state.input_state,
        key,
        in_terminal,
        popup_showing,
        false,
        state.macros.recording.is_some(),
    );
    state.input_state = new_input_state;
    state.execute(cmd, screen_area)
}

/// Process a mouse event.
fn process_mouse_event(
    state: &mut EditorState,
    kind: &str,
    col: u16,
    row: u16,
    screen_area: novim_types::Rect,
) {
    use crossterm::event::{MouseEvent, MouseEventKind, MouseButton};

    let mouse_kind = match kind {
        "LeftDown" => MouseEventKind::Down(MouseButton::Left),
        "ScrollUp" => MouseEventKind::ScrollUp,
        "ScrollDown" => MouseEventKind::ScrollDown,
        _ => return,
    };

    let mouse = MouseEvent {
        kind: mouse_kind,
        column: col,
        row,
        modifiers: KeyModifiers::empty(),
    };

    state.handle_mouse(mouse, screen_area);
}
