// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// // SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end TUI tests using tuiprobe — busx runs as a real subprocess
//! in a PTY, driven by keyboard events, with output verified via
//! terminal emulation (alacritty_terminal).

use std::time::Duration;

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, TuiProbe};

fn spawn_busx(addr: &str, w: u16, h: u16) -> TuiProbe {
    let mut probe = TuiProbe::builder()
        .cols(w)
        .rows(h)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut cmd = CommandBuilder::new(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("busx"),
    );
    cmd.arg("--address");
    cmd.arg(addr);
    probe.spawn(cmd).unwrap();
    probe
}

#[test]
fn service_list_renders() {
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 12);

    probe.wait_for_text("org.busx.ScrollA").unwrap();
    assert!(probe.contains("Services"));

    probe.send_key(KeyCode::Char('q')).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(!probe.is_running());
}

#[test]
fn enter_drills_into_objects() {
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    // Wait for service list.
    probe.wait_for_text("org.busx.ScrollA").unwrap();

    // Down to org.busx.Test (sorts after ScrollA-L = 12 Down).
    for _ in 0..12 {
        probe.send_key(KeyCode::Down).unwrap();
    }
    probe.wait_for_text("org.busx.Test").unwrap();

    // Enter — this was the test that exposed ratatui-testlib's fatal
    // reader bug (grid rebuild was garbled). With alacritty_terminal
    // the diff rendering should be correct.
    probe.send_key(KeyCode::Enter).unwrap();
    probe.wait_for_text("/org/busx/Test").unwrap();

    // Verify the screen is NOT garbled — the path should be clean
    // without leftover service-name fragments.
    let screen = probe.screen_contents();
    assert!(
        screen.contains("/org/busx/Test"),
        "objects screen should show the path: {screen}"
    );
    assert!(
        !screen.contains("ScrollB"),
        "no stale service-name fragments: {screen}"
    );

    probe.send_key(KeyCode::Char('q')).unwrap();
}
