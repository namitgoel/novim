//! Terminal grid — a 2D array of styled cells representing a virtual screen.

/// ANSI color
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CellColor {
    #[default]
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

/// Saved state for the main screen when alternate screen is active.
struct SavedScreen {
    cells: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
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
    /// When true, the next printable character wraps to the next line first.
    /// This implements "pending wrap" behavior: writing at the last column
    /// does NOT immediately move the cursor — only the *next* character does.
    wrap_pending: bool,
    /// Saved main-screen buffer (when alternate screen is active).
    saved_screen: Option<SavedScreen>,
    /// Scroll region: top and bottom row (inclusive). None = full screen.
    scroll_top: usize,
    scroll_bottom: usize,
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
            wrap_pending: false,
            saved_screen: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
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

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn wrap_pending(&self) -> bool {
        self.wrap_pending
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
        self.wrap_pending = false;
    }

    pub fn move_cursor_down(&mut self, n: usize) {
        self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
        self.wrap_pending = false;
    }

    pub fn move_cursor_forward(&mut self, n: usize) {
        self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
        self.wrap_pending = false;
    }

    pub fn move_cursor_back(&mut self, n: usize) {
        self.cursor_col = self.cursor_col.saturating_sub(n);
        self.wrap_pending = false;
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
        self.wrap_pending = false;
    }

    // --- Content operations ---

    pub fn put_char(&mut self, c: char) {
        // If a wrap is pending from writing the last column, wrap now.
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cursor_col = 0;
            self.newline();
        }
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            self.cells[self.cursor_row][self.cursor_col] = Cell {
                c,
                fg: self.pen_fg,
                bg: self.pen_bg,
                attrs: self.pen_attrs,
            };
            if self.cursor_col + 1 >= self.cols {
                // At last column — set pending wrap instead of wrapping immediately.
                self.wrap_pending = true;
            } else {
                self.cursor_col += 1;
            }
        }
    }

    pub fn newline(&mut self) {
        if self.cursor_row >= self.scroll_bottom {
            // Scroll the scroll region up by one line.
            if self.scroll_top < self.rows && self.scroll_bottom < self.rows {
                self.cells.remove(self.scroll_top);
                self.cells.insert(self.scroll_bottom, vec![Cell::default(); self.cols]);
            }
        } else {
            self.cursor_row += 1;
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.wrap_pending = false;
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

    // --- Alternate screen buffer ---

    /// Switch to the alternate screen buffer (saves main screen + cursor).
    pub fn enter_alternate_screen(&mut self) {
        if self.saved_screen.is_some() {
            return; // already on alternate screen
        }
        self.saved_screen = Some(SavedScreen {
            cells: self.cells.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        });
        self.clear_all();
        self.set_cursor(0, 0);
    }

    /// Switch back to the main screen buffer (restores saved screen + cursor).
    pub fn leave_alternate_screen(&mut self) {
        if let Some(saved) = self.saved_screen.take() {
            self.cells = saved.cells;
            self.cursor_row = saved.cursor_row.min(self.rows.saturating_sub(1));
            self.cursor_col = saved.cursor_col.min(self.cols.saturating_sub(1));
            // Resize if dimensions changed while on alternate screen.
            self.cells.resize_with(self.rows, || vec![Cell::default(); self.cols]);
            for row in &mut self.cells {
                row.resize_with(self.cols, Cell::default);
            }
        }
    }

    // --- Scroll region ---

    /// Set scroll region (DECSTBM). Both `top` and `bottom` are 0-based inclusive.
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let top = top.min(self.rows.saturating_sub(1));
        let bottom = bottom.min(self.rows.saturating_sub(1));
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        }
        self.set_cursor(0, 0);
    }

    /// Reset scroll region to full screen.
    pub fn reset_scroll_region(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
    }

    // --- Line insert / delete ---

    /// Insert `n` blank lines at the cursor row (within scroll region). Lines at
    /// the bottom of the scroll region are discarded.
    pub fn insert_lines(&mut self, n: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        for _ in 0..n {
            if self.scroll_bottom < self.rows {
                self.cells.remove(self.scroll_bottom);
            }
            self.cells.insert(self.cursor_row, vec![Cell::default(); self.cols]);
        }
        self.cursor_col = 0;
    }

    /// Delete `n` lines at the cursor row (within scroll region). Blank lines
    /// are added at the bottom of the scroll region.
    pub fn delete_lines(&mut self, n: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        for _ in 0..n {
            self.cells.remove(self.cursor_row);
            self.cells.insert(self.scroll_bottom, vec![Cell::default(); self.cols]);
        }
        self.cursor_col = 0;
    }

    // --- Character insert / delete ---

    /// Delete `n` characters at the cursor position, shifting the rest left.
    pub fn delete_chars(&mut self, n: usize) {
        if self.cursor_row < self.rows {
            let row = &mut self.cells[self.cursor_row];
            let start = self.cursor_col;
            for _ in 0..n {
                if start < row.len() {
                    row.remove(start);
                    row.push(Cell::default());
                }
            }
        }
    }

    /// Insert `n` blank characters at the cursor position, shifting the rest right.
    pub fn insert_chars(&mut self, n: usize) {
        if self.cursor_row < self.rows {
            let row = &mut self.cells[self.cursor_row];
            let start = self.cursor_col;
            for _ in 0..n {
                if start < row.len() {
                    row.pop();
                    row.insert(start, Cell::default());
                }
            }
        }
    }

    // --- Erase characters ---

    /// Erase `n` characters from cursor position (replace with blanks, don't shift).
    pub fn erase_chars(&mut self, n: usize) {
        if self.cursor_row < self.rows {
            for col in self.cursor_col..(self.cursor_col + n).min(self.cols) {
                self.cells[self.cursor_row][col] = Cell::default();
            }
        }
    }

    /// Clear from beginning of line to cursor (inclusive).
    pub fn clear_to_start_of_line(&mut self) {
        if self.cursor_row < self.rows {
            for col in 0..=self.cursor_col.min(self.cols.saturating_sub(1)) {
                self.cells[self.cursor_row][col] = Cell::default();
            }
        }
    }

    /// Clear from beginning of screen to cursor (inclusive).
    pub fn clear_to_start_of_screen(&mut self) {
        self.clear_to_start_of_line();
        for row in 0..self.cursor_row {
            self.clear_line(row);
        }
    }

    // --- Resize ---

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.cells.resize_with(rows, || vec![Cell::default(); cols]);
        for row in &mut self.cells {
            row.resize_with(cols, Cell::default);
        }
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        // Reset scroll region to full screen on resize.
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
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
