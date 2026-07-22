// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

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
    KeyCode, KeyModifiers, MouseButton, ScrollDirection, encode_key, encode_mouse, encode_scroll,
};
pub use harness::{TuiProbe, TuiProbeBuilder};
pub use pty::Pty;

/// Wait until `probe`'s screen matches the insta snapshot named `name`.
///
/// Drives `probe.wait_for` with a condition that wraps the public
/// `insta::assert_snapshot!` macro in `catch_unwind`: a match does not panic
/// (condition true → `wait_for` returns `Ok`); a mismatch panics, which
/// `catch_unwind` catches (condition false → keep polling); a timeout becomes
/// `Error::Timeout`. This reuses insta's whole pipeline — filter (the caller's
/// thread-local `Settings`), snapshot load/compare, and `.snap.new` writing —
/// so there is no fragile substring match and no hand-rolled filter.
///
/// The macro expands at the call site, so `file!()` / `module_path!()` /
/// `line!()` resolve to the caller (the test crate), and snapshots land in the
/// caller's `tests/snapshots/` directory. `insta` itself is resolved at the
/// call site too, so the caller crate must have insta available (busx has it
/// as a dev-dependency).
///
/// ```no_run
/// # use tuiprobe::{TuiProbe, wait_for_snapshot};
/// # let mut probe = TuiProbe::new(80, 24).unwrap();
/// wait_for_snapshot!(&mut probe, "my_screen").unwrap();
/// ```
#[macro_export]
macro_rules! wait_for_snapshot {
    ($probe:expr, $name:expr $(,)?) => {
        $probe.wait_for(|screen| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ::insta::assert_snapshot!($name, screen);
            }))
            .is_ok()
        })
    };
}
