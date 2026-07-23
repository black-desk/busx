// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use std::thread;
use std::time::Duration;

/// `emit` with no args succeeds (broadcasts a signal whose body is empty).
#[test]
fn emit_no_body_succeeds() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "emit",
            "/org/busx/Test",
            "org.busx.Test",
            "Poked",
            "", // empty signature → empty body
        ])
        .assert()
        .success();
}

/// A broadcast `emit` is capturable by `monitor` on the same bus (default
/// signal subscription). This is the real end-to-end proof the signal left
/// busx and crossed the bus.
#[test]
fn emit_signal_is_captured_by_monitor() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let addr2 = addr.clone();
    let monitor = thread::spawn(move || {
        Command::cargo_bin("busx")
            .unwrap()
            .args([
                "--address",
                &addr2,
                "monitor",
                "--interface",
                "org.busx.Test",
                "--limit-messages",
                "1",
                "--timeout",
                "5",
            ])
            .ok()
            .unwrap()
    });

    thread::sleep(Duration::from_millis(400));
    Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "emit",
            "/org/busx/Test",
            "org.busx.Test",
            "Poked",
            "",
        ])
        .assert()
        .success();

    let out = monitor.join().unwrap();
    assert!(out.status.success(), "monitor should exit 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Poked"),
        "monitor should capture the emitted signal: {stdout}"
    );
}
