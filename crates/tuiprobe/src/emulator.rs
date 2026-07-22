// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

//! Terminal emulator backed by [`alacritty_terminal::Term`].
//!
//! Feeding raw PTY bytes into a `Term` reconstructs the visible screen grid
//! — the same code path Alacritty uses to render its own terminal window.
//! This gives us production-grade ANSI escape parsing (CUP, SGR, EL, ED,
//! alternate screen, cursor save/restore, scroll, etc.) with zero bugs to
//! maintain ourselves.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, Term};
use vte::ansi;

/// Simple dimension provider for [`Term::new`].
struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// A terminal screen that processes PTY output and maintains the grid state.
///
/// Use [`Screen::feed`] to push raw bytes, [`Screen::contents`] to read the
/// rendered text, and [`Screen::cell`] to inspect individual cells
/// (character, colors, attributes).
pub struct Screen {
    term: Term<VoidListener>,
    processor: ansi::Processor,
}

impl Screen {
    /// Create a new screen with the given dimensions (no scrollback —
    /// suitable for testing where you only care about the visible viewport).
    pub fn new(cols: u16, rows: u16) -> Self {
        let term = Term::new(
            Config::default(),
            &Size {
                cols: cols as usize,
                rows: rows as usize,
            },
            VoidListener,
        );
        Screen {
            term,
            processor: ansi::Processor::new(),
        }
    }

    /// Feed raw PTY output bytes into the terminal emulator. Each byte is
    /// processed through the VTE parser, which dispatches to `Term`'s
    /// `Handler` impl (updating the grid, cursor, colors, etc.).
    pub fn feed(&mut self, data: &[u8]) {
        self.processor.advance(&mut self.term, data);
    }

    /// Return the full visible screen as a string — each row on its own line.
    ///
    /// Trailing whitespace on each line is trimmed so snapshots are clean;
    /// the last line has no trailing newline.
    pub fn contents(&self) -> String {
        let lines = self.term.screen_lines();
        let cols = self.term.columns();
        let mut out = String::with_capacity(lines * cols);
        for row in 0..lines {
            let mut line_end = cols;
            // Trim trailing spaces for clean snapshots.
            for col in (0..cols).rev() {
                let c = self.cell(row, col).c;
                if c != ' ' {
                    line_end = col + 1;
                    break;
                }
            }
            for col in 0..line_end {
                let c = self.cell(row, col).c;
                out.push(c);
            }
            if row + 1 < lines {
                out.push('\n');
            }
        }
        out
    }

    /// Check whether the screen text contains `needle` (anywhere, any row).
    pub fn contains(&self, needle: &str) -> bool {
        self.contents().contains(needle)
    }

    /// Access the cell at `(row, col)` — 0-based.
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        let grid: &Grid<Cell> = self.term.grid();
        let point = Point::new(Line(row as i32), Column(col));
        &grid[point]
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.term.columns()
    }

    /// Number of visible rows.
    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }
}
