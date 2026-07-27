// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression for the TUI logging lifecycle.
//!
//! `log::init_tui` returns a `WorkerGuard` whose drop flushes and joins the
//! non-blocking writer thread. `main` must keep that guard alive for the whole
//! `tui::run`; if it is dropped right after `init_tui` returns, every
//! `tracing` event emitted during the run is silently lost and the TUI log
//! file stays empty regardless of `-v`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin;

#[test]
fn tui_keeps_log_guard_alive_across_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("busx.log");

    // Point both bus addresses at nonexistent sockets so `connect_with_bus`:
    //   1. fails to reach the session bus, then emits the fallback debug line
    //      (`session bus unavailable … falling back to system bus`), and
    //   2. fails to reach the system bus too, so the process errors out and
    //      exits on its own (the terminal is never put in raw mode, so there
    //      is no hang and no need for a PTY).
    let mut cmd = Command::new(cargo_bin("busx"));
    cmd.args(["-vv", "--log"]).arg(&log);
    cmd.env(
        "DBUS_SESSION_BUS_ADDRESS",
        "unix:path=/nonexistent/busx-test-session",
    );
    cmd.env(
        "DBUS_SYSTEM_BUS_ADDRESS",
        "unix:path=/nonexistent/busx-test-system",
    );
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn busx");

    // The child should exit on its own (connect failure). Backstop: if it is
    // somehow kept alive past the deadline, kill it so the test fails instead
    // of hanging the whole suite.
    let deadline = Instant::now() + Duration::from_secs(10);
    let exited = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break false,
        }
    };

    let content = std::fs::read_to_string(&log).unwrap_or_default();

    // The fallback debug event fires inside `tui::run` (after `init_tui`), so
    // it lands in the log file only if `main` kept the WorkerGuard alive past
    // `init_tui`.
    assert!(
        content.contains("falling back to system bus"),
        "TUI log is missing the session→system fallback debug line; the \
         `init_tui` WorkerGuard was likely dropped before `tui::run`. \
         (exited={exited})\n--- log ---\n{content}"
    );
}
