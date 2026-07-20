// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Logging via `tracing`. Two entry points chosen by the run mode:
//!
//! - [`init_cli`]: a stderr formatter (CLI subcommands print diagnostics to
//!   stderr; capture with `2> file`).
//! - [`init_tui`]: a non-blocking file appender. The TUI owns the terminal in
//!   raw mode + the alternate screen, so it must never write to the TTY —
//!   diagnostics go to `$XDG_CACHE_HOME/busx/busx.log` (or `--log=<path>`).
//!
//! Verbosity is the `-v` repeat count: default WARN, `-v` INFO, `-vv` DEBUG,
//! `-vvv` TRACE. `RUST_LOG` overrides the count when set (handy for ad-hoc
//! debugging). Installing a subscriber also captures zbus's own `tracing`
//! events — valuable for a D-Bus tool (visible at `-vvv`).
//!
//! Both functions are no-ops if a global subscriber is already installed, so
//! the completion path (which runs before normal parsing and must stay silent)
//! is unaffected: with no subscriber, `tracing` macros compile to nothing.

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Map a `-v` repeat count to a max-level directive string.
fn level_from_verbose(v: u8) -> &'static str {
    match v {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    }
}

/// Build the level filter: `RUST_LOG` wins if set, else the `-v` count.
fn filter(verbose: u8) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level_from_verbose(verbose)))
}

/// Initialise logging for a CLI run: a stderr formatter at the `-v`-implied
/// level. Idempotent — a no-op if a subscriber is already installed.
pub fn init_cli(verbose: u8) {
    tracing_subscriber::registry()
        .with(filter(verbose))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init()
        .ok();
}

/// Initialise logging for a TUI run: a non-blocking writer thread appending to
/// a file (so the TTY is never touched). Returns a [`WorkerGuard`] that the
/// caller must keep alive until exit — dropping it flushes and joins the
/// writer thread.
pub fn init_tui(
    verbose: u8,
    log_path: Option<&str>,
) -> Result<tracing_appender::non_blocking::WorkerGuard, std::io::Error> {
    let path = log_path.map(PathBuf::from).unwrap_or_else(default_log_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::registry()
        .with(filter(verbose))
        // No ANSI colours in a log file.
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .try_init()
        .ok();
    Ok(guard)
}

/// `$XDG_CACHE_HOME/busx/busx.log`, falling back to `~/.cache/busx/busx.log`
/// when `XDG_CACHE_HOME` is unset. A missing `HOME` degrades to a relative
/// `busx.log` in the current directory.
fn default_log_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("busx").join("busx.log")
}
