//! VTE Performer — handles ANSI escape sequences by updating the grid.

use vte::{Params, Perform};

use super::grid::{CellAttrs, CellColor, Grid};

pub struct GridPerformer<'a> {
    pub grid: &'a mut Grid,
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
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let p1 = params.first().copied().unwrap_or(1) as usize;
        let p2 = params.get(1).copied().unwrap_or(1) as usize;

        match action {
            'A' => self.grid.move_cursor_up(p1),
            'B' => self.grid.move_cursor_down(p1),
            'C' => self.grid.move_cursor_forward(p1),
            'D' => self.grid.move_cursor_back(p1),
            'H' | 'f' => self.grid.set_cursor(p1.saturating_sub(1), p2.saturating_sub(1)),
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.grid.clear_to_end_of_screen(),
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
                    2 => self.grid.clear_line(self.grid.cursor_row()),
                    _ => {}
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
                    let mut attrs = CellAttrs::default();
                    attrs.bold = true;
                    self.grid.set_pen_attrs(attrs);
                }
                2 => {
                    let mut attrs = CellAttrs::default();
                    attrs.dim = true;
                    self.grid.set_pen_attrs(attrs);
                }
                4 => {
                    let mut attrs = CellAttrs::default();
                    attrs.underline = true;
                    self.grid.set_pen_attrs(attrs);
                }
                7 => {
                    let mut attrs = CellAttrs::default();
                    attrs.reverse = true;
                    self.grid.set_pen_attrs(attrs);
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
                    // Extended foreground: 38;5;N (256-color)
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.grid.set_pen_fg(CellColor::Indexed(params[i + 2] as u8));
                        i += 2;
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
                    // Extended background: 48;5;N (256-color)
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.grid.set_pen_bg(CellColor::Indexed(params[i + 2] as u8));
                        i += 2;
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
