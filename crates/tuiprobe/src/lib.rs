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
//! probe.wait_for(|s| s.contains("Welcome"))?;
//!
//! // Navigate.
//! probe.send_key(KeyCode::Down);
//! probe.send_key(KeyCode::Enter);
//! probe.wait_for(|s| s.contains("Settings"))?;
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
/// Polls with [`insta::matches_snapshot!`], which compares the screen against
/// the stored snapshot **with no side effects** — it applies the caller's
/// thread-local `Settings` (filters/redactions/suffix) and the same comparison
/// logic as `assert_snapshot!`, but never writes a `.snap.new`, prints, or
/// panics. So the many intermediate frames seen while waiting for the screen
/// to converge do not litter pending snapshots (the failure mode of the old
/// `catch_unwind`-around-`assert_snapshot!` approach).
///
/// If the screen never matches within the probe's timeout, one final real
/// [`insta::assert_snapshot!`] is run on the current screen — that *does*
/// write the `.snap.new` for review and panics with the actual diff. So a
/// `.snap.new` is produced only for a genuine, terminal mismatch, never for
/// transient polling frames.
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
    ($probe:expr, $name:expr $(,)?) => {{
        // Evaluate the name once and hold it as a borrowed `&str` so the poll
        // closure (which `wait_for` calls many times as `Fn`) can capture it
        // by `Copy`, and so `String` / `format!(...)` names don't get moved.
        // `$probe` is used directly (not rebound) so it is borrowed, not moved.
        let name_owned: String = ::std::format!("{}", $name);
        let name = name_owned.as_str();
        match $probe.wait_for(|screen| ::insta::matches_snapshot!(name, screen).unwrap_or(false)) {
            Ok(()) => Ok(()),
            // Timed out: the screen never converged. Run one real assert so
            // insta writes the `.snap.new` for review and reports the diff.
            Err(err) => {
                let screen = $probe.screen_contents();
                ::insta::assert_snapshot!(name, screen.as_str());
                // assert_snapshot! panics on mismatch; if execution reaches
                // here (e.g. insta force-pass), surface the timeout error.
                Err(err)
            }
        }
    }};
}
