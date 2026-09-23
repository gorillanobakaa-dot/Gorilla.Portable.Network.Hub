// Version: 1.0.0 · updated 26-09-23-14-00
//
// Long lists laid out in columns that fill the window, and moved through a
// page at a time.
//
// WHY THIS EXISTS. The list of what gets handed out was one column of names
// cut at 32 characters, on a screen 240 columns wide, and it did not scroll:
// "and 34 more not shown, the window is too short", with the cursor going
// down into rows nobody could see. Reported by the owner, 2026-09-23, with
// the point that a person who does not know terminals would never think to
// maximise the window, and would be no better off if they did.
//
// So: as many columns as the window holds, each wide enough to read a name;
// names cut in the middle so the file name survives; a page that follows the
// cursor; and a line that says in words where you are and which key shows
// the rest. Reading order is down the first column, then the next, as in a
// file manager's list view and `ls`.

use crate::term::{self, Key};

/// Columns keep at least this much room for a name, so filling a wide
/// window never means a wall of unreadable stubs.
const MIN_COL: usize = 28;
/// And at most this much, so one very long name does not take a whole
/// wide window for itself.
const MAX_COL: usize = 60;
const GAP: usize = 3;
const MARGIN: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// How many items there are.
    pub n: usize,
    /// Columns on screen at once.
    pub cols: usize,
    /// Width of one column's text.
    pub colw: usize,
    /// Items down each column.
    pub per_col: usize,
    /// The first column on screen. Moves to follow the cursor.
    pub first_col: usize,
}

impl Grid {
    /// Lay out `n` items, the widest `widest` columns, into a window
    /// `win_cols` wide with `rows` lines for the list, keeping `sel` on
    /// screen and moving as little as possible from `prev_first`.
    pub fn lay(n: usize, widest: usize, win_cols: usize, rows: usize, sel: usize, prev_first: usize) -> Grid {
        let rows = rows.max(1);
        let usable = win_cols.saturating_sub(MARGIN * 2).max(8);
        let (cols, colw, per_col) = if n <= rows {
            // Everything fits in one column: give names the whole width.
            (1, widest.min(usable).max(1), n.max(1))
        } else {
            let colw = widest.clamp(MIN_COL, MAX_COL).min(usable);
            let fit = ((usable + GAP) / (colw + GAP)).max(1);
            let needed = n.div_ceil(rows);
            if needed <= fit {
                // Fits on one page: spread evenly rather than leaving the
                // last column a stub.
                (needed, colw, n.div_ceil(needed))
            } else {
                (fit, colw, rows)
            }
        };
        let total = n.div_ceil(per_col).max(1);
        let c = sel.min(n.saturating_sub(1)) / per_col;
        let mut first = prev_first.min(total.saturating_sub(cols));
        if c < first {
            first = c;
        } else if c >= first + cols {
            first = c + 1 - cols;
        }
        Grid { n, cols, colw, per_col, first_col: first }
    }

    /// Columns in the whole list, on screen or not.
    pub fn total_cols(&self) -> usize {
        self.n.div_ceil(self.per_col).max(1)
    }

    /// Whether everything is on screen at once.
    pub fn all_visible(&self) -> bool {
        self.first_col == 0 && self.total_cols() <= self.cols
    }

    /// The first item on screen and one past the last.
    pub fn shown(&self) -> (usize, usize) {
        let a = self.first_col * self.per_col;
        let b = ((self.first_col + self.cols) * self.per_col).min(self.n);
        (a.min(self.n), b)
    }

    /// Where a key moves the cursor, or None when the key is not a move here
    /// (so the caller can give it another meaning, as the picker does with
    /// left and right in a single column).
    pub fn step(&self, k: Key, sel: usize) -> Option<usize> {
        if self.n == 0 {
            return None;
        }
        let last = self.n - 1;
        let page = self.per_col * self.cols;
        let multi = self.total_cols() > 1;
        match k {
            Key::Up => Some(sel.saturating_sub(1)),
            Key::Down => Some((sel + 1).min(last)),
            Key::Left if multi => Some(if sel >= self.per_col { sel - self.per_col } else { sel }),
            Key::Right if multi => {
                if sel + self.per_col <= last {
                    Some(sel + self.per_col)
                } else if sel / self.per_col < last / self.per_col {
                    // The next column is shorter than this row: land on its end.
                    Some(last)
                } else {
                    Some(sel)
                }
            }
            Key::PageDown => Some((sel + page).min(last)),
            Key::PageUp => Some(sel.saturating_sub(page)),
            Key::Home => Some(0),
            Key::End => Some(last),
            _ => None,
        }
    }

    /// The lines to draw, with the selected cell in reverse video. Each line
    /// is at most MARGIN + cols*colw + gaps wide, which `lay` kept inside the
    /// window, so they go to the frame whole.
    pub fn lines(&self, cells: &[String], sel: Option<usize>) -> Vec<String> {
        let mut out = Vec::new();
        for r in 0..self.per_col {
            let mut line = " ".repeat(MARGIN);
            let mut any = false;
            for c in self.first_col..self.first_col + self.cols {
                let i = c * self.per_col + r;
                if i >= self.n || i >= cells.len() {
                    break;
                }
                if c > self.first_col {
                    line.push_str(&" ".repeat(GAP));
                }
                let text = term::truncate_middle(&cells[i], self.colw);
                let pad = " ".repeat(self.colw.saturating_sub(term::width(&text)));
                if sel == Some(i) {
                    line.push_str(&format!("\x1b[7m{text}{pad}\x1b[0m"));
                } else {
                    line.push_str(&text);
                    line.push_str(&pad);
                }
                any = true;
            }
            if any {
                out.push(line.trim_end().to_string());
            }
        }
        out
    }

    /// Where you are and how to see the rest, in words. None when it all
    /// fits, because then there is nothing to say.
    pub fn status(&self) -> Option<String> {
        if self.all_visible() {
            return None;
        }
        let (a, b) = self.shown();
        let mut s = format!("Showing {} to {} of {}.", a + 1, b, self.n);
        let more_right = self.first_col + self.cols < self.total_cols();
        let more_left = self.first_col > 0;
        if more_right && self.cols > 1 {
            s.push_str(" More to the right: press the right arrow or Page Down.");
        } else if more_right {
            s.push_str(" More below: press the down arrow or Page Down.");
        }
        if more_left {
            s.push_str(" Earlier ones: Page Up.");
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported case: 78 names in a window 240 wide and ~40 rows tall.
    /// Everything must be on screen, in several columns, none of them stubs.
    #[test]
    fn a_wide_window_shows_the_whole_list_in_columns() {
        let g = Grid::lay(78, 45, 240, 40, 0, 0);
        assert!(g.cols >= 2, "{g:?}");
        assert!(g.colw >= MIN_COL);
        assert!(g.all_visible(), "{g:?}");
        assert_eq!(g.status(), None);
        assert!(MARGIN + g.cols * g.colw + (g.cols - 1) * GAP <= 240);
    }

    /// Too many for one page: the cursor's page is shown, and the words say
    /// which key shows the rest.
    #[test]
    fn a_long_list_pages_and_says_how_to_see_more() {
        let g = Grid::lay(1000, 40, 120, 20, 0, 0);
        assert!(!g.all_visible());
        let s = g.status().unwrap();
        assert!(s.starts_with("Showing 1 to "), "{s}");
        assert!(s.contains("Page Down"), "{s}");
        // Moving to the end brings the last page on screen.
        let end = g.step(Key::End, 0).unwrap();
        let g2 = Grid::lay(1000, 40, 120, 20, end, g.first_col);
        let (a, b) = g2.shown();
        assert!(a <= end && end < b, "{g2:?}");
        assert!(g2.status().unwrap().contains("Page Up"));
    }

    /// The old fault: the cursor could go where nothing was drawn. Every
    /// position the keys can reach must be on screen after laying out again.
    #[test]
    fn the_cursor_is_never_off_screen() {
        let mut sel = 0;
        let mut first = 0;
        for k in [Key::Down, Key::Right, Key::PageDown, Key::Right, Key::End, Key::Left, Key::PageUp, Key::Up, Key::Home] {
            for _ in 0..7 {
                let g = Grid::lay(500, 30, 100, 12, sel, first);
                sel = g.step(k, sel).unwrap_or(sel);
                let g = Grid::lay(500, 30, 100, 12, sel, g.first_col);
                first = g.first_col;
                let (a, b) = g.shown();
                assert!(a <= sel && sel < b, "{k:?} put {sel} outside {a}..{b}");
            }
        }
    }

    /// In one column left and right are not moves, so the picker can keep
    /// them for going up and into folders.
    #[test]
    fn left_and_right_are_free_in_a_single_column() {
        let g = Grid::lay(5, 20, 80, 20, 0, 0);
        assert_eq!(g.cols, 1);
        assert_eq!(g.step(Key::Left, 2), None);
        assert_eq!(g.step(Key::Right, 2), None);
        assert_eq!(g.step(Key::Down, 2), Some(3));
    }

    #[test]
    fn a_long_path_keeps_its_file_name() {
        let cut = term::truncate_middle("Lessons/Year 7/science/week 3/notes.txt", 28);
        assert!(cut.ends_with("notes.txt"), "{cut}");
        assert!(cut.starts_with("Lessons"), "{cut}");
        assert!(term::width(&cut) <= 28);
    }

    #[test]
    fn the_selected_cell_is_marked_and_lines_fit() {
        let cells: Vec<String> = (0..10).map(|i| format!("file-{i}.txt")).collect();
        let g = Grid::lay(10, 12, 80, 4, 5, 0);
        let lines = g.lines(&cells, Some(5));
        assert!(lines.iter().any(|l| l.contains("\x1b[7mfile-5.txt")));
        for l in &lines {
            let visible = l.replace("\x1b[7m", "").replace("\x1b[0m", "");
            assert!(term::width(&visible) <= 80);
        }
    }
}
