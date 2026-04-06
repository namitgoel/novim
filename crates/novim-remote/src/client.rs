//! Remote client: runs locally, connects to a remote novim server via SSH.
//!
//! Spawns `ssh user@host novim serve`, captures local input,
//! sends it to the remote, receives rendered frames, paints them.

use std::io::{self, BufReader, BufWriter, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::{
    cursor, execute, queue,
    event::{self, Event, KeyCode, KeyEvent},
    style::{self, Attribute, Color, SetBackgroundColor, SetForegroundColor, SetAttribute},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::protocol::*;
use crate::transport;

/// Connect to a remote machine via SSH and run the novim client.
pub fn connect(destination: &str, path: Option<&str>) -> io::Result<()> {
    // Build the remote command
    let mut remote_args = vec!["novim".to_string(), "serve".to_string()];
    if let Some(p) = path {
        remote_args.push("--path".to_string());
        remote_args.push(p.to_string());
    }

    // Spawn SSH process
    let mut child = Command::new("ssh")
        .arg(destination)
        .args(&remote_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // show SSH errors directly
        .spawn()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to spawn ssh: {}", e)))?;

    let ssh_stdin = child.stdin.take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No stdin on SSH process"))?;
    let ssh_stdout = child.stdout.take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No stdout on SSH process"))?;

    let mut writer = BufWriter::new(ssh_stdin);
    let mut reader = BufReader::new(ssh_stdout);

    // Get terminal size BEFORE entering raw mode
    let (term_width, term_height) = terminal::size()?;

    // Send Hello and wait for HelloAck BEFORE entering raw mode
    // This way if SSH fails, the terminal stays clean
    transport::write_message(&mut writer, &ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        width: term_width,
        height: term_height,
    })?;

    let ack: ServerMessage = transport::read_message(&mut reader)
        .ok_or_else(|| {
            let _ = child.wait();
            io::Error::new(io::ErrorKind::ConnectionRefused, "No HelloAck from server — is novim installed on the remote machine?")
        })?;
    match ack {
        ServerMessage::HelloAck { .. } => {}
        ServerMessage::Error { message } => {
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::Other, format!("Server error: {}", message)));
        }
        _ => {
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::Other, "Unexpected server response"));
        }
    }

    eprintln!("[novim-ssh] Connected! Setting up terminal...");

    // NOW setup local terminal (only after successful handshake)
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;

    // Start reader thread: reads ServerMessages from SSH stdout
    let (tx, rx) = mpsc::channel::<ServerMessage>();
    thread::spawn(move || {
        loop {
            match transport::read_message::<ServerMessage>(&mut reader) {
                Some(msg) => {
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
                None => break, // EOF — SSH disconnected
            }
        }
    });

    // Main event loop
    let result = run_client_loop(&mut writer, &rx, &mut stdout);

    // Cleanup
    cleanup_terminal();
    let _ = child.wait();

    result
}

/// Main client event loop.
fn run_client_loop(
    writer: &mut BufWriter<impl Write>,
    rx: &mpsc::Receiver<ServerMessage>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    loop {
        // 1. Check for received frames from server
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ServerMessage::Frame { cells, cursor } => {
                    let rows = cells.len();
                    let cols = cells.first().map(|r| r.len()).unwrap_or(0);
                    log::info!("[novim-ssh] Got frame: {}x{}", cols, rows);
                    paint_frame(stdout, &cells, cursor)?;
                }
                ServerMessage::Delta { changes, cursor } => {
                    paint_delta(stdout, &changes, cursor)?;
                }
                ServerMessage::Bye => {
                    return Ok(());
                }
                ServerMessage::Pong => {}
                ServerMessage::Error { message } => {
                    // Show error briefly then continue
                    log::error!("Server error: {}", message);
                }
                ServerMessage::HelloAck { .. } => {} // ignore
            }
        }

        // 2. Check for local input
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    let code = key_to_string(&key);
                    let mods = key.modifiers.bits() as u8;
                    transport::write_message(writer, &ClientMessage::Key { code, modifiers: mods })?;
                }
                Event::Mouse(mouse) => {
                    let kind = match mouse.kind {
                        event::MouseEventKind::Down(event::MouseButton::Left) => "LeftDown",
                        event::MouseEventKind::ScrollUp => "ScrollUp",
                        event::MouseEventKind::ScrollDown => "ScrollDown",
                        _ => continue,
                    };
                    transport::write_message(writer, &ClientMessage::Mouse {
                        kind: kind.to_string(),
                        col: mouse.column,
                        row: mouse.row,
                        modifiers: mouse.modifiers.bits() as u8,
                    })?;
                }
                Event::Resize(w, h) => {
                    transport::write_message(writer, &ClientMessage::Resize { width: w, height: h })?;
                }
                _ => {}
            }
        }

        // 3. Check if SSH process died (rx disconnected)
        // The reader thread will close the channel on EOF
    }
}

/// Paint a full frame to the terminal.
fn paint_frame(
    stdout: &mut impl Write,
    cells: &[Vec<StyledCell>],
    cursor: Option<(u16, u16)>,
) -> io::Result<()> {
    queue!(stdout, cursor::Hide)?;

    for (y, row) in cells.iter().enumerate() {
        queue!(stdout, cursor::MoveTo(0, y as u16))?;
        for cell in row {
            let fg = Color::Rgb { r: cell.fg[0], g: cell.fg[1], b: cell.fg[2] };
            let bg = Color::Rgb { r: cell.bg[0], g: cell.bg[1], b: cell.bg[2] };
            queue!(stdout, SetForegroundColor(fg), SetBackgroundColor(bg))?;

            if cell.attrs & 0b0001 != 0 { queue!(stdout, SetAttribute(Attribute::Bold))?; }
            if cell.attrs & 0b0010 != 0 { queue!(stdout, SetAttribute(Attribute::Dim))?; }
            if cell.attrs & 0b0100 != 0 { queue!(stdout, SetAttribute(Attribute::Underlined))?; }
            if cell.attrs & 0b1000 != 0 { queue!(stdout, SetAttribute(Attribute::Reverse))?; }

            queue!(stdout, style::Print(cell.c))?;

            if cell.attrs != 0 { queue!(stdout, SetAttribute(Attribute::Reset))?; }
        }
    }

    if let Some((x, y)) = cursor {
        queue!(stdout, cursor::MoveTo(x, y), cursor::Show)?;
    }

    stdout.flush()
}

/// Paint delta changes to the terminal.
fn paint_delta(
    stdout: &mut impl Write,
    changes: &[(u16, u16, StyledCell)],
    cursor: Option<(u16, u16)>,
) -> io::Result<()> {
    for (x, y, cell) in changes {
        let fg = Color::Rgb { r: cell.fg[0], g: cell.fg[1], b: cell.fg[2] };
        let bg = Color::Rgb { r: cell.bg[0], g: cell.bg[1], b: cell.bg[2] };
        queue!(
            stdout,
            cursor::MoveTo(*x, *y),
            SetForegroundColor(fg),
            SetBackgroundColor(bg),
            style::Print(cell.c),
        )?;
    }

    if let Some((x, y)) = cursor {
        queue!(stdout, cursor::MoveTo(x, y), cursor::Show)?;
    }

    stdout.flush()
}

/// Serialize a key event to a string representation.
fn key_to_string(key: &KeyEvent) -> String {
    match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::F(n) => format!("F{}", n),
        _ => "Unknown".to_string(),
    }
}

/// Cleanup terminal on exit.
fn cleanup_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        event::DisableMouseCapture,
        LeaveAlternateScreen,
    );
}
