//! VTE Performer — handles ANSI escape sequences by updating the grid.

use vte::{Params, Perform};

use super::grid::{CellAttrs, CellColor, Grid};

pub struct GridPerformer<'a> {
    pub grid: &'a mut Grid,
    /// Accumulated responses to write back to the PTY (e.g. DSR replies).
    pub responses: Vec<Vec<u8>>,
}

impl<'a> Perform for GridPerformer<'a> {
    fn print(&mut self, c: char) {
        self.grid.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => self.grid.backspace(),
            0x09 => self.grid.tab(),
            0x0A => self.grid.newline(),
            0x0D => self.grid.carriage_return(),
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(first) = params.first() {
            // OSC 133 — Shell integration / prompt markers
            // Format: \x1b]133;A\x07 (prompt start), B (command start), C (output start), D (done)
            if *first == b"133" {
                if let Some(type_byte) = params.get(1) {
                    if !type_byte.is_empty() {
                        match type_byte[0] {
                            b'A' => {
                                // Prompt start — record this position for prompt navigation
                                self.grid.add_prompt_mark();
                            }
                            // B, C, D are recognized but no action needed yet
                            _ => {}
                        }
                    }
                }
                return;
            }

            // OSC 8 — Hyperlinks
            // Format: \x1b]8;params;uri\x07 (open) or \x1b]8;;\x07 (close)
            if *first == b"8" {
                // params[1] = optional params (id=...), params[2] = URI
                // If URI is empty, this closes the hyperlink
                if let Some(uri_bytes) = params.get(2) {
                    if let Ok(uri) = std::str::from_utf8(uri_bytes) {
                        if uri.is_empty() {
                            self.grid.active_hyperlink = None;
                        } else {
                            self.grid.active_hyperlink = Some(uri.to_string());
                        }
                    }
                } else if params.len() == 2 {
                    // OSC 8;; with no third param = close
                    self.grid.active_hyperlink = None;
                }
                return;
            }
        }

        // OSC 7 — Shell CWD integration
        // Format: \x1b]7;file:///path\x07  or  \x1b]7;file://host/path\x07
        // VTE splits on ';' so params[0] = b"7", params[1] = b"file:///path"
        if let Some(first) = params.first() {
            if *first == b"7" {
                if let Some(uri_bytes) = params.get(1) {
                    if let Ok(uri) = std::str::from_utf8(uri_bytes) {
                        // Strip "file://" prefix, optionally with hostname
                        let path = if let Some(rest) = uri.strip_prefix("file://") {
                            // rest is either "/path" or "hostname/path"
                            if rest.starts_with('/') {
                                rest.to_string()
                            } else {
                                // Skip hostname part: find the first '/'
                                rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_default()
                            }
                        } else {
                            uri.to_string()
                        };
                        if !path.is_empty() {
                            self.grid.osc7_cwd = Some(path);
                        }
                    }
                }
            }
        }
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let p1 = params.first().copied().unwrap_or(1) as usize;
        let p2 = params.get(1).copied().unwrap_or(1) as usize;

        // DEC Private Mode Set/Reset: CSI ? <mode> h/l
        let is_private = intermediates.first() == Some(&b'?');
        if is_private {
            match action {
                'h' => {
                    // Set mode
                    for &p in &params {
                        match p {
                            47 | 1047 | 1049 => self.grid.enter_alternate_screen(),
                            25 => {} // show cursor (handled by renderer)
                            _ => {}
                        }
                    }
                    return;
                }
                'l' => {
                    // Reset mode
                    for &p in &params {
                        match p {
                            47 | 1047 | 1049 => self.grid.leave_alternate_screen(),
                            25 => {} // hide cursor (handled by renderer)
                            _ => {}
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        // Bail out for any other intermediate-bearing sequences we don't handle.
        if !intermediates.is_empty() {
            // CSI > c  → DA2 (Secondary Device Attributes)
            if intermediates == [b'>'] && action == 'c' {
                // Report as VT220: CSI > 1;10;0c
                self.responses.push(b"\x1b[>1;10;0c".to_vec());
            }
            return;
        }

        match action {
            'A' => self.grid.move_cursor_up(p1),
            'B' => self.grid.move_cursor_down(p1),
            'C' => self.grid.move_cursor_forward(p1),
            'D' => self.grid.move_cursor_back(p1),
            'H' | 'f' => self.grid.set_cursor(p1.saturating_sub(1), p2.saturating_sub(1)),
            'G' | '`' => {
                // CHA — Cursor Character Absolute (column only)
                let col = p1.saturating_sub(1);
                self.grid.set_cursor(self.grid.cursor_row(), col);
            }
            'd' => {
                // VPA — Line Position Absolute (row only)
                let row = p1.saturating_sub(1);
                self.grid.set_cursor(row, self.grid.cursor_col());
            }
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.grid.clear_to_end_of_screen(),
                    1 => self.grid.clear_to_start_of_screen(),
                    2 | 3 => {
                        self.grid.clear_all();
                        self.grid.set_cursor(0, 0);
                    }
                    _ => {}
                }
            }
            'K' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.grid.clear_to_end_of_line(),
                    1 => self.grid.clear_to_start_of_line(),
                    2 => self.grid.clear_line(self.grid.cursor_row()),
                    _ => {}
                }
            }
            'L' => self.grid.insert_lines(p1),
            'M' => self.grid.delete_lines(p1),
            'P' => self.grid.delete_chars(p1),
            '@' => self.grid.insert_chars(p1),
            'X' => self.grid.erase_chars(p1),
            'r' => {
                // DECSTBM — Set scrolling region
                let top = params.first().copied().unwrap_or(1) as usize;
                let bottom = params.get(1).copied().unwrap_or(self.grid.rows() as u16) as usize;
                self.grid.set_scroll_region(top.saturating_sub(1), bottom.saturating_sub(1));
            }
            'n' => {
                // DSR — Device Status Report
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    5 => {
                        // Status report — respond "OK"
                        self.responses.push(b"\x1b[0n".to_vec());
                    }
                    6 => {
                        // Cursor Position Report — respond with current position (1-based)
                        let row = self.grid.cursor_row() + 1;
                        let col = self.grid.cursor_col() + 1;
                        self.responses.push(format!("\x1b[{};{}R", row, col).into_bytes());
                    }
                    _ => {}
                }
            }
            'c' => {
                // DA — Device Attributes (primary)
                if params.first().copied().unwrap_or(0) == 0 {
                    // Report as VT220
                    self.responses.push(b"\x1b[?62;1;2;6;7;8;9c".to_vec());
                }
            }
            // SGR — Select Graphic Rendition (colors & styles)
            'm' => self.handle_sgr(&params),
            _ => {}
        }
    }
}

impl<'a> GridPerformer<'a> {
    fn handle_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.grid.reset_pen();
            return;
        }

        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.grid.reset_pen(),
                1 => {
                    self.grid.set_pen_attrs(CellAttrs { bold: true, ..CellAttrs::default() });
                }
                2 => {
                    self.grid.set_pen_attrs(CellAttrs { dim: true, ..CellAttrs::default() });
                }
                4 => {
                    self.grid.set_pen_attrs(CellAttrs { underline: true, ..CellAttrs::default() });
                }
                7 => {
                    self.grid.set_pen_attrs(CellAttrs { reverse: true, ..CellAttrs::default() });
                }
                22 => self.grid.set_pen_attrs(CellAttrs::default()), // Normal intensity
                24 => self.grid.set_pen_attrs(CellAttrs::default()), // No underline
                27 => self.grid.set_pen_attrs(CellAttrs::default()), // No reverse

                // Foreground colors
                30 => self.grid.set_pen_fg(CellColor::Black),
                31 => self.grid.set_pen_fg(CellColor::Red),
                32 => self.grid.set_pen_fg(CellColor::Green),
                33 => self.grid.set_pen_fg(CellColor::Yellow),
                34 => self.grid.set_pen_fg(CellColor::Blue),
                35 => self.grid.set_pen_fg(CellColor::Magenta),
                36 => self.grid.set_pen_fg(CellColor::Cyan),
                37 => self.grid.set_pen_fg(CellColor::White),
                38 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color: 38;5;N
                        self.grid.set_pen_fg(CellColor::Indexed(params[i + 2] as u8));
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // 24-bit true color: 38;2;R;G;B
                        let r = params[i + 2] as u8;
                        let g = params[i + 3] as u8;
                        let b = params[i + 4] as u8;
                        self.grid.set_pen_fg(CellColor::Rgb(r, g, b));
                        i += 4;
                    }
                }
                39 => self.grid.set_pen_fg(CellColor::Default),

                // Background colors
                40 => self.grid.set_pen_bg(CellColor::Black),
                41 => self.grid.set_pen_bg(CellColor::Red),
                42 => self.grid.set_pen_bg(CellColor::Green),
                43 => self.grid.set_pen_bg(CellColor::Yellow),
                44 => self.grid.set_pen_bg(CellColor::Blue),
                45 => self.grid.set_pen_bg(CellColor::Magenta),
                46 => self.grid.set_pen_bg(CellColor::Cyan),
                47 => self.grid.set_pen_bg(CellColor::White),
                48 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        // 256-color: 48;5;N
                        self.grid.set_pen_bg(CellColor::Indexed(params[i + 2] as u8));
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        // 24-bit true color: 48;2;R;G;B
                        let r = params[i + 2] as u8;
                        let g = params[i + 3] as u8;
                        let b = params[i + 4] as u8;
                        self.grid.set_pen_bg(CellColor::Rgb(r, g, b));
                        i += 4;
                    }
                }
                49 => self.grid.set_pen_bg(CellColor::Default),

                // Bright foreground
                90 => self.grid.set_pen_fg(CellColor::BrightBlack),
                91 => self.grid.set_pen_fg(CellColor::BrightRed),
                92 => self.grid.set_pen_fg(CellColor::BrightGreen),
                93 => self.grid.set_pen_fg(CellColor::BrightYellow),
                94 => self.grid.set_pen_fg(CellColor::BrightBlue),
                95 => self.grid.set_pen_fg(CellColor::BrightMagenta),
                96 => self.grid.set_pen_fg(CellColor::BrightCyan),
                97 => self.grid.set_pen_fg(CellColor::BrightWhite),

                // Bright background
                100 => self.grid.set_pen_bg(CellColor::BrightBlack),
                101 => self.grid.set_pen_bg(CellColor::BrightRed),
                102 => self.grid.set_pen_bg(CellColor::BrightGreen),
                103 => self.grid.set_pen_bg(CellColor::BrightYellow),
                104 => self.grid.set_pen_bg(CellColor::BrightBlue),
                105 => self.grid.set_pen_bg(CellColor::BrightMagenta),
                106 => self.grid.set_pen_bg(CellColor::BrightCyan),
                107 => self.grid.set_pen_bg(CellColor::BrightWhite),

                _ => {}
            }
            i += 1;
        }
    }
}
