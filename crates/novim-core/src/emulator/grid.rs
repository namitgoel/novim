//! Terminal grid — a 2D array of styled cells representing a virtual screen.

/// ANSI color
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
}

impl Default for CellColor {
    fn default() -> Self {
        CellColor::Default
    }
}

/// Cell attributes
#[derive(Clone, Copy, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
    pub reverse: bool,
}

/// A single cell in the terminal grid.
#[derive(Clone)]
pub struct Cell {
    pub c: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub attrs: CellAttrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: CellColor::Default,
            bg: CellColor::Default,
            attrs: CellAttrs::default(),
        }
    }
}

/// A virtual terminal screen.
pub struct Grid {
    cells: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    rows: usize,
    cols: usize,
    /// Current pen style (applied to new characters)
    pen_fg: CellColor,
    pen_bg: CellColor,
    pen_attrs: CellAttrs,
}

impl Grid {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            cells: vec![vec![Cell::default(); cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
            pen_fg: CellColor::Default,
            pen_bg: CellColor::Default,
            pen_attrs: CellAttrs::default(),
        }
    }

    // --- Accessors ---

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    // --- Pen / style ---

    pub fn set_pen_fg(&mut self, color: CellColor) {
        self.pen_fg = color;
    }

    pub fn set_pen_bg(&mut self, color: CellColor) {
        self.pen_bg = color;
    }

    pub fn set_pen_attrs(&mut self, attrs: CellAttrs) {
        self.pen_attrs = attrs;
    }

    pub fn reset_pen(&mut self) {
        self.pen_fg = CellColor::Default;
        self.pen_bg = CellColor::Default;
        self.pen_attrs = CellAttrs::default();
    }

    // --- Cursor movement ---

    pub fn move_cursor_up(&mut self, n: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(n);
    }

    pub fn move_cursor_down(&mut self, n: usize) {
        self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
    }

    pub fn move_cursor_forward(&mut self, n: usize) {
        self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
    }

    pub fn move_cursor_back(&mut self, n: usize) {
        self.cursor_col = self.cursor_col.saturating_sub(n);
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
    }

    // --- Content operations ---

    pub fn put_char(&mut self, c: char) {
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            self.cells[self.cursor_row][self.cursor_col] = Cell {
                c,
                fg: self.pen_fg,
                bg: self.pen_bg,
                attrs: self.pen_attrs,
            };
            self.cursor_col += 1;
            if self.cursor_col >= self.cols {
                self.cursor_col = 0;
                self.newline();
            }
        }
    }

    pub fn newline(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.cells.remove(0);
            self.cells.push(vec![Cell::default(); self.cols]);
        } else {
            self.cursor_row += 1;
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    pub fn tab(&mut self) {
        let next_tab = (self.cursor_col / 8 + 1) * 8;
        self.cursor_col = next_tab.min(self.cols.saturating_sub(1));
    }

    // --- Clearing ---

    pub fn clear_to_end_of_line(&mut self) {
        if self.cursor_row < self.rows {
            for col in self.cursor_col..self.cols {
                self.cells[self.cursor_row][col] = Cell::default();
            }
        }
    }

    pub fn clear_line(&mut self, row: usize) {
        if row < self.rows {
            for col in 0..self.cols {
                self.cells[row][col] = Cell::default();
            }
        }
    }

    pub fn clear_to_end_of_screen(&mut self) {
        self.clear_to_end_of_line();
        for row in (self.cursor_row + 1)..self.rows {
            self.clear_line(row);
        }
    }

    pub fn clear_all(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = Cell::default();
            }
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.cells.resize_with(rows, || vec![Cell::default(); cols]);
        for row in &mut self.cells {
            row.resize_with(cols, Cell::default);
        }
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    // --- Rendering ---

    pub fn get_line(&self, row: usize) -> String {
        if row < self.rows {
            self.cells[row]
                .iter()
                .map(|c| c.c)
                .collect::<String>()
                .trim_end()
                .to_string()
        } else {
            String::new()
        }
    }

    /// Get cells for a row (for styled rendering).
    pub fn get_cells(&self, row: usize) -> Option<&[Cell]> {
        if row < self.rows {
            Some(&self.cells[row])
        } else {
            None
        }
    }
}
