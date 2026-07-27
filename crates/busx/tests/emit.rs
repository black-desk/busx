// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command as StdCommand, Stdio};

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

/// A broadcast `emit` is capturable by `monitor` on the same bus (`--type
/// signal` subscription). This is the real end-to-end proof the signal left
/// busx and crossed the bus. The emit fires only after `monitor` reports it is
/// live on the bus (its `{"event":"ready"}` line), so there is no sleep race
/// that could lose the signal.
#[test]
fn emit_signal_is_captured_by_monitor() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("busx"))
        .args([
            "--json",
            "--address",
            &addr,
            "monitor",
            "--type",
            "signal",
            "--interface",
            "org.busx.Test",
            "--limit-messages",
            "1",
            "--timeout",
            "5s",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    // Read monitor stdout line by line; the moment it reports it is live, fire
    // the emit. No fixed sleep.
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = BufReader::new(stdout);
    let mut ready = false;
    let mut collected = String::new();
    for line in reader.lines() {
        let line = line.expect("read monitor line");
        if !ready && line.contains("\"event\":\"ready\"") {
            ready = true;
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
        }
        collected.push_str(&line);
        collected.push('\n');
    }
    assert!(ready, "monitor never went live");

    let status = child.wait().expect("monitor exit");
    assert!(status.success(), "monitor should exit 0: {status}");

    let first: Value = serde_json::from_str(
        collected
            .lines()
            .find(|l| !l.contains("\"event\":\"ready\""))
            .unwrap_or(""),
    )
    .expect("captured line must be JSON:\n{collected}");
    assert_eq!(first["type"], "signal", "expected a signal:\n{collected}");
    assert_eq!(
        first["member"], "Poked",
        "monitor should capture the emitted signal:\n{collected}"
    );
}
