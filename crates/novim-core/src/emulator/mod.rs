//! Terminal emulator — PTY + VTE parser + Grid
//!
//! Spawns a shell in a PTY, reads output on a background thread,
//! parses ANSI sequences via VTE, and renders to a Grid.

pub mod grid;
mod performer;

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread;

/// Global waker callback — set by the GUI to wake the event loop when PTY data arrives.
static PTY_WAKER: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Register a waker that will be called from the PTY reader thread whenever new
/// data is available.  The GUI sets this to an `EventLoopProxy::send_event` call.
pub fn set_pty_waker(waker: Arc<dyn Fn() + Send + Sync>) {
    PTY_WAKER.set(waker).ok();
}

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use vte::Parser;

use novim_types::{Direction, Position};

use crate::buffer::{PaneDisplay, TextEditing, Searchable, TerminalLike};
use grid::Grid;
use performer::GridPerformer;

/// A terminal pane that runs a shell via PTY.
pub struct TerminalPane {
    grid: Grid,
    parser: Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    receiver: Receiver<Vec<u8>>,
    _reader_handle: thread::JoinHandle<()>,
    child_pid: Option<u32>,
}

impl TerminalPane {
    /// Spawn a new terminal pane with the given size.
    pub fn new(rows: u16, cols: u16) -> io::Result<Self> {
        let pty_system = native_pty_system();

        let pty_pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;

        // Spawn the user's shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");

        let child = pty_pair
            .slave
            .spawn_command(cmd)
            .map_err(io::Error::other)?;

        // Get child PID for cwd detection
        let child_pid = child.process_id();

        drop(pty_pair.slave);

        let writer = pty_pair
            .master
            .take_writer()
            .map_err(io::Error::other)?;

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();

        // Background thread: read PTY output → send to main thread
        let mut reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(io::Error::other)?;

        let waker = PTY_WAKER.get().cloned();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 65536]; // 64 KB — large reads reduce syscall overhead
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF or error, shell exited
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // Receiver dropped
                        }
                        // Wake the event loop so it polls immediately.
                        if let Some(ref w) = waker {
                            w();
                        }
                    }
                }
            }
        });

        Ok(Self {
            grid: Grid::new(rows as usize, cols as usize),
            parser: Parser::new(),
            master: pty_pair.master,
            writer,
            receiver: rx,
            _reader_handle: handle,
            child_pid,
        })
    }

    /// Get the shell's current working directory.
    /// Uses /proc on Linux, proc_pidinfo on macOS (instant, no lsof).
    pub fn shell_cwd(&self) -> Option<std::path::PathBuf> {
        let pid = self.child_pid?;

        #[cfg(target_os = "macos")]
        {
            use std::ffi::CStr;
            use std::mem;
            use std::os::raw::c_int;

            // PROC_PIDVNODEPATHINFO = 9
            const PROC_PIDVNODEPATHINFO: c_int = 9;
            const MAXPATHLEN: usize = 1024;

            #[repr(C)]
            struct VnodeInfoPath {
                _vip_vi: [u8; 152], // struct vnode_info (padding)
                vip_path: [u8; MAXPATHLEN],
            }

            #[repr(C)]
            struct PidVnodePathInfo {
                pvi_cdir: VnodeInfoPath,
                _pvi_rdir: VnodeInfoPath,
            }

            extern "C" {
                fn proc_pidinfo(
                    pid: c_int,
                    flavor: c_int,
                    arg: u64,
                    buffer: *mut std::ffi::c_void,
                    buffersize: c_int,
                ) -> c_int;
            }

            unsafe {
                let mut info: PidVnodePathInfo = mem::zeroed();
                let size = mem::size_of::<PidVnodePathInfo>() as c_int;
                let ret = proc_pidinfo(
                    pid as c_int,
                    PROC_PIDVNODEPATHINFO,
                    0,
                    &mut info as *mut _ as *mut std::ffi::c_void,
                    size,
                );

                if ret <= 0 {
                    return None;
                }

                let cstr = CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const _);
                let path = cstr.to_str().ok()?;
                if path.is_empty() {
                    None
                } else {
                    Some(std::path::PathBuf::from(path))
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }

    /// Process any pending PTY output — call this in the event loop.
    /// Returns `true` if any data was consumed.
    pub fn poll_pty(&mut self) -> bool {
        // Process at most 64 pending chunks per poll to avoid blocking the
        // event loop when a command produces large amounts of output.
        let mut consumed = false;
        let mut budget = 64;
        while budget > 0 {
            match self.receiver.try_recv() {
                Ok(data) => {
                    let mut performer = GridPerformer {
                        grid: &mut self.grid,
                        responses: Vec::new(),
                    };
                    for byte in &data {
                        self.parser.advance(&mut performer, *byte);
                    }
                    // Flush any responses (e.g. DSR cursor position) back to the PTY.
                    if !performer.responses.is_empty() {
                        for response in performer.responses.drain(..) {
                            let _ = self.writer.write_all(&response);
                        }
                        let _ = self.writer.flush();
                    }
                    consumed = true;
                    budget -= 1;
                }
                Err(_) => break,
            }
        }
        consumed
    }

    /// Send a byte to the PTY (forward keyboard input).
    pub fn write_to_pty(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
    }

    /// Send a single key as bytes to the PTY.
    pub fn send_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        let bytes: Option<Vec<u8>> = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+letter → send as control character
                    let ctrl = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                    Some(vec![ctrl])
                } else {
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    Some(s.as_bytes().to_vec())
                }
            }
            KeyCode::Enter => Some(vec![b'\r']),
            KeyCode::Backspace => Some(vec![0x7f]),
            KeyCode::Tab => Some(vec![b'\t']),
            KeyCode::Esc => Some(vec![0x1b]),
            KeyCode::Left => Some(b"\x1b[D".to_vec()),
            KeyCode::Right => Some(b"\x1b[C".to_vec()),
            KeyCode::Up => Some(b"\x1b[A".to_vec()),
            KeyCode::Down => Some(b"\x1b[B".to_vec()),
            _ => None,
        };

        if let Some(data) = bytes {
            self.write_to_pty(&data);
        }
    }

    /// Resize the PTY and grid.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.grid.resize(rows as usize, cols as usize);
    }
}

impl PaneDisplay for TerminalPane {
    fn cursor(&self) -> Position {
        Position::new(self.grid.cursor_row(), self.grid.cursor_col())
    }

    fn move_cursor(&mut self, direction: Direction) {
        match direction {
            Direction::Up => self.write_to_pty(b"\x1b[A"),
            Direction::Down => self.write_to_pty(b"\x1b[B"),
            Direction::Right => self.write_to_pty(b"\x1b[C"),
            Direction::Left => self.write_to_pty(b"\x1b[D"),
        }
    }

    fn get_line(&self, line: usize) -> Option<String> {
        if line < self.grid.rows() {
            Some(self.grid.get_line(line))
        } else {
            None
        }
    }

    fn len_lines(&self) -> usize {
        self.grid.rows()
    }

    fn display_name(&self) -> String {
        "[Terminal]".to_string()
    }

    fn is_dirty(&self) -> bool {
        false
    }
}

/// Terminal uses default (no-op) text editing and search.
impl TextEditing for TerminalPane {}
impl Searchable for TerminalPane {}

impl TerminalLike for TerminalPane {
    fn send_key(&mut self, key: crossterm::event::KeyEvent) {
        TerminalPane::send_key(self, key);
    }

    fn is_terminal(&self) -> bool {
        true
    }

    fn poll_pty(&mut self) -> bool {
        TerminalPane::poll_pty(self)
    }

    fn shell_cwd(&self) -> Option<std::path::PathBuf> {
        TerminalPane::shell_cwd(self)
    }

    fn get_styled_cells(&self, row: usize) -> Option<&[grid::Cell]> {
        self.grid.get_cells(row)
    }
}
