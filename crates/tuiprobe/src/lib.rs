// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// // SPDX-License-Identifier: MIT

//! tuiprobe — PTY-based integration testing for TUI applications.
//!
//! Spin up a TUI app (ratatui, cursive, raw crossterm, …) as a child process
//! inside a pseudo-terminal, send keyboard / mouse events, wait for the
//! rendered output to reach a known state, and snapshot it.
//!
//! The terminal emulator is [`alacritty_terminal::Term`] — the same engine
//! that powers the Alacritty terminal emulator. All ANSI escape sequences
//! (cursor movement, SGR colors, erase operations, alternate screen, …) are
//! handled with production-grade correctness.
//!
//! # Quick start
//!
//! ```no_run
//! use portable_pty::CommandBuilder;
//! use tuiprobe::{KeyCode, TuiProbe};
//!
//! let mut probe = TuiProbe::new(80, 24)?;
//! probe.spawn(CommandBuilder::new("my-tui-app"))?;
//!
//! // Wait for the app to render its initial screen.
//! probe.wait_for_text("Welcome")?;
//!
//! // Navigate.
//! probe.send_key(KeyCode::Down);
//! probe.send_key(KeyCode::Enter);
//! probe.wait_for_text("Settings")?;
//!
//! // Snapshot the rendered terminal.
//! println!("{}", probe.screen_contents());
//!
//! # Ok::<(), tuiprobe::Error>(())
//! ```

mod emulator;
mod error;
mod events;
mod harness;
mod pty;

pub use emulator::Screen;
pub use error::{Error, Result};
pub use events::{
    encode_key, encode_mouse, encode_scroll, KeyCode, KeyModifiers, MouseButton,
    ScrollDirection,
};
pub use harness::{TuiProbe, TuiProbeBuilder};
pub use pty::Pty;
