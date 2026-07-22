// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

//! High-level test harness: combines PTY management + terminal emulation
//! into an ergonomic API for driving TUI applications.
//!
//! ```no_run
//! use portable_pty::CommandBuilder;
//! use tuiprobe::{TuiProbe, KeyCode};
//!
//! let mut probe = TuiProbe::new(80, 24)?;
//! probe.spawn(CommandBuilder::new("my-tui-app"))?;
//! probe.wait_for_text("Ready")?;
//! probe.send_key(KeyCode::Enter);
//! probe.wait_for_text("Results")?;
//! println!("{}", probe.screen_contents());
//! # Ok::<(), tuiprobe::Error>(())
//! ```

use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, ExitStatus};

use crate::emulator::Screen;
use crate::error::{Error, Result};
use crate::events::{
    KeyCode, KeyModifiers, MouseButton, ScrollDirection, encode_key, encode_mouse, encode_scroll,
};
use crate::pty::Pty;

/// A PTY-based test harness for TUI applications.
///
/// Spawns the app as a child process in a pseudo-terminal, feeds keyboard /
/// mouse input, polls the rendered output, and waits for conditions.
pub struct TuiProbe {
    pty: Option<Pty>,
    screen: Screen,
    timeout: Duration,
    poll_interval: Duration,
}

/// Builder for customizing [`TuiProbe`] settings.
pub struct TuiProbeBuilder {
    cols: u16,
    rows: u16,
    timeout: Duration,
    poll_interval: Duration,
}

impl TuiProbe {
    /// Create a harness with the given terminal size and default settings
    /// (5s timeout, 10ms poll).
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        Self::builder().cols(cols).rows(rows).build()
    }

    /// Start a builder for custom configuration.
    pub fn builder() -> TuiProbeBuilder {
        TuiProbeBuilder {
            cols: 80,
            rows: 24,
            timeout: Duration::from_secs(5),
            poll_interval: Duration::from_millis(10),
        }
    }

    /// Spawn a child process in the PTY.
    pub fn spawn(&mut self, cmd: CommandBuilder) -> Result<()> {
        let pty = Pty::spawn(cmd, self.screen.cols() as u16, self.screen.rows() as u16)?;
        self.pty = Some(pty);
        Ok(())
    }

    // ── Input ──────────────────────────────────────────────────────────

    /// Send a single key press (no modifiers).
    pub fn send_key(&mut self, key: KeyCode) -> Result<()> {
        self.send_key_with_mods(key, KeyModifiers::NONE)
    }

    /// Send a key with modifier flags (Ctrl/Alt/Shift).
    pub fn send_key_with_mods(&mut self, key: KeyCode, mods: KeyModifiers) -> Result<()> {
        let bytes = encode_key(key, mods);
        self.write(&bytes)?;
        // Brief pause so the child's event loop can process the input and
        // render a frame before we proceed.
        std::thread::sleep(Duration::from_millis(20));
        self.drain_into_emulator();
        Ok(())
    }

    /// Type a string of printable characters (one key event per char).
    pub fn send_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            self.send_key(KeyCode::Char(ch))?;
        }
        Ok(())
    }

    /// Simulate a mouse button press + release at `(col, row)` (0-based).
    pub fn mouse_click(&mut self, col: u16, row: u16, button: MouseButton) -> Result<()> {
        let press = encode_mouse(col, row, button, true);
        let release = encode_mouse(col, row, button, false);
        self.write(&press)?;
        self.write(&release)?;
        std::thread::sleep(Duration::from_millis(20));
        self.drain_into_emulator();
        Ok(())
    }

    /// Simulate a mouse scroll notch at `(col, row)`.
    pub fn mouse_scroll(&mut self, col: u16, row: u16, dir: ScrollDirection) -> Result<()> {
        let bytes = encode_scroll(col, row, dir);
        self.write(&bytes)?;
        std::thread::sleep(Duration::from_millis(20));
        self.drain_into_emulator();
        Ok(())
    }

    // ── Waiting ────────────────────────────────────────────────────────

    /// Wait until the screen contents satisfy `condition`. Polls at the
    /// configured interval. Returns [`Error::Timeout`] if the deadline is
    /// reached (the error includes a screen dump for debugging).
    pub fn wait_for<F: Fn(&str) -> bool>(&mut self, condition: F) -> Result<()> {
        self.wait_for_with_timeout(condition, self.timeout)
    }

    /// Like [`wait_for`](Self::wait_for) but with a custom timeout.
    pub fn wait_for_with_timeout<F: Fn(&str) -> bool>(
        &mut self,
        condition: F,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            self.drain_into_emulator();
            let contents = self.screen.contents();
            if condition(&contents) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    what: "condition".to_string(),
                    screen: contents,
                });
            }
            std::thread::sleep(self.poll_interval);
        }
    }

    /// Wait until `text` appears anywhere on the screen.
    pub fn wait_for_text(&mut self, text: &str) -> Result<()> {
        let needle = text.to_string();
        let result = self.wait_for(move |s| s.contains(&needle));
        if result.is_err() {
            return Err(Error::Timeout {
                what: format!("text: {text}"),
                screen: self.screen.contents(),
            });
        }
        Ok(())
    }

    /// Wait until `text` appears, with a custom timeout.
    pub fn wait_for_text_timeout(&mut self, text: &str, timeout: Duration) -> Result<()> {
        let text = text.to_string();
        self.wait_for_with_timeout(move |s| s.contains(&text), timeout)
    }

    // ── Output ─────────────────────────────────────────────────────────

    /// Return the full visible screen as a trimmed string (rows joined by
    /// `\n`, trailing spaces removed per line).
    pub fn screen_contents(&self) -> String {
        self.screen.contents()
    }

    /// Check whether the screen currently contains `text`.
    pub fn contains(&self, text: &str) -> bool {
        self.screen.contains(text)
    }

    // ── Process control ───────────────────────────────────────────────

    /// Whether the child process is still running.
    pub fn is_running(&mut self) -> bool {
        self.pty.as_mut().is_some_and(|p| p.is_running())
    }

    /// Block until the child exits.
    pub fn wait_exit(&mut self) -> Result<ExitStatus> {
        self.pty.as_mut().ok_or(Error::ProcessExited)?.wait_exit()
    }

    /// Resize the PTY window.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        if let Some(pty) = &mut self.pty {
            pty.resize(cols, rows)?;
        }
        self.screen = Screen::new(cols, rows);
        Ok(())
    }

    // ── Internals ──────────────────────────────────────────────────────

    fn write(&mut self, data: &[u8]) -> Result<()> {
        match &mut self.pty {
            Some(pty) => pty.write(data),
            None => Err(Error::ProcessExited),
        }
    }

    fn drain_into_emulator(&mut self) {
        if let Some(pty) = &mut self.pty {
            let data = pty.drain();
            if !data.is_empty() {
                self.screen.feed(&data);
            }
        }
    }
}

impl TuiProbeBuilder {
    pub fn cols(mut self, cols: u16) -> Self {
        self.cols = cols;
        self
    }
    pub fn rows(mut self, rows: u16) -> Self {
        self.rows = rows;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }
    pub fn build(self) -> Result<TuiProbe> {
        Ok(TuiProbe {
            pty: None,
            screen: Screen::new(self.cols, self.rows),
            timeout: self.timeout,
            poll_interval: self.poll_interval,
        })
    }
}
