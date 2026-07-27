// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::cargo_bin;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// First stdout line that is an actual captured message, skipping the
/// `{"event":"ready",...}` (JSON mode) or `busx: monitoring ...` (human mode)
/// line `monitor` emits once it is live on the bus.
fn first_message_line(stdout: &str) -> &str {
    stdout.lines().find(|l| !is_ready_line(l)).unwrap_or("")
}

/// All captured-message lines, skipping the ready line (see
/// [`first_message_line`]).
fn message_lines(stdout: &str) -> Vec<&str> {
    stdout.lines().filter(|l| !is_ready_line(l)).collect()
}

/// Is `line` monitor's ready event (JSON or human form)?
fn is_ready_line(line: &str) -> bool {
    line.contains("\"event\":\"ready\"") || line.starts_with("busx: monitoring")
}

/// `busx monitor --type signal ... --limit-messages 1` must
/// emit one NDJSON line for the `PropertiesChanged` signal triggered by a
/// property set.
///
/// This is a concurrent test: the monitor subscribes first, then a second
/// `busx call` mutates the `volume` property (the fixture emits
/// `org.freedesktop.DBus.Properties.PropertiesChanged`), and the monitor's
/// captured stdout must contain the matching line.
#[test]
fn monitor_emits_propertieschanged() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    // Start monitor as a subprocess; it exits after 1 matching message
    // (`--limit-messages`). A `--timeout` backstop keeps the test from hanging
    // if the signal is missed (it should never fire in the happy path).
    let child = Command::new(cargo_bin!("busx"))
        .args([
            "--json",
            "--address",
            &addr,
            "monitor",
            "--type",
            "signal",
            "--interface",
            "org.freedesktop.DBus.Properties",
            "--member",
            "PropertiesChanged",
            "--limit-messages",
            "1",
            "--timeout",
            "10s",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    // Give the monitor time to register its match rule on the bus.
    thread::sleep(Duration::from_millis(800));

    // Trigger a property change: `busx set` calls `Properties.Set`, which
    // routes through the fixture's generated `set_volume` setter. zbus
    // auto-emits `PropertiesChanged` for properties with the default
    // `emits_changed_signal` (the fixture's `volume` qualifies).
    let trigger = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "set",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "volume",
            "d",
            "0.75",
        ])
        .status()
        .expect("trigger set");
    assert!(trigger.success(), "set volume call failed");

    let out = child.wait_with_output().expect("monitor exit");
    assert!(out.status.success(), "monitor failed: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Each line must be a JSON object whose `member` is PropertiesChanged.
    // (stdout also carries the `{"event":"ready",...}` line emitted once the
    // subscription is live; first_message_line skips it.)
    let lines = message_lines(&stdout);
    assert!(!lines.is_empty(), "monitor produced no output:\n{stdout}");

    let first: Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|e| panic!("first line is not JSON ({e}):\n{stdout}"));
    assert_eq!(first["type"], "signal", "expected a signal:\n{stdout}");
    assert_eq!(
        first["member"], "PropertiesChanged",
        "expected PropertiesChanged:\n{stdout}"
    );
    assert_eq!(
        first["interface"], "org.freedesktop.DBus.Properties",
        "wrong interface:\n{stdout}"
    );
    // The receipt timestamp must be an epoch-seconds float.
    assert!(
        first["ts"].as_f64().is_some_and(|t| t > 1_000_000_000.0),
        "ts not a plausible epoch float:\n{stdout}"
    );
    // PropertiesChanged body signature is `sa{sv}as`.
    assert_eq!(first["signature"], "sa{sv}as", "wrong signature:\n{stdout}");

    // Only one line because of --limit-messages 1.
    assert_eq!(lines.len(), 1, "expected exactly one line:\n{stdout}");
}

/// Human `monitor` (no `--json`) emits a multi-line block per message instead
/// of NDJSON: the first line names the type, the second carries member/serial,
/// then each body argument. `set` triggers a `PropertiesChanged` signal.
#[test]
fn monitor_human_emits_block() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let child = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "monitor",
            "--type",
            "signal",
            "--interface",
            "org.freedesktop.DBus.Properties",
            "--member",
            "PropertiesChanged",
            "--limit-messages",
            "1",
            "--timeout",
            "10s",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    thread::sleep(Duration::from_millis(800));

    let trigger = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "set",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "volume",
            "d",
            "0.5",
        ])
        .status()
        .expect("trigger set");
    assert!(trigger.success(), "set volume call failed");

    let out = child.wait_with_output().expect("monitor exit");
    assert!(out.status.success(), "monitor failed: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // The block must NOT be JSON (no leading `{`) and must carry the signal's
    // identity fields. stdout also carries the `busx: monitoring ...` ready
    // line; the first *message* line names the message type.
    let first_line = stdout
        .lines()
        .find(|l| !l.starts_with("busx: monitoring"))
        .unwrap_or("");
    assert!(
        first_line.starts_with("signal"),
        "human block should start with `signal`:\n{stdout}"
    );
    assert!(
        stdout.contains("member=PropertiesChanged"),
        "missing member line:\n{stdout}"
    );
    assert!(
        stdout.contains("org.busx.Test"),
        "missing changed interface in body:\n{stdout}"
    );
    assert!(
        !stdout.trim_start().starts_with('{'),
        "human mode must not emit JSON:\n{stdout}"
    );
}

/// Regression: `--timeout` must fire even when **no matching traffic** arrives.
/// Before the async `MessageStream` rewrite the blocking iterator's `next()`
/// dead-waited, so the deadline check (inside the loop body) only ran after a
/// message landed — `--timeout` hung forever on an idle bus. This guards that.
#[test]
fn monitor_timeout_fires_on_idle_bus() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    // Subscribe to a sender that never emits on the test bus → the match-rule
    // stream yields nothing, so the ONLY way this process returns is the
    // `--timeout` firing.
    let mut child = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "monitor",
            "--sender",
            ":1.999999", // a unique name that will never speak on the test bus
            "--timeout",
            "500ms",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    // Poll with a hard kill deadline so a regression FAILS instead of hanging
    // the whole suite: the old code would block forever here.
    let kill = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "monitor failed: {status}");
                break;
            }
            Ok(None) if Instant::now() >= kill => {
                let _ = child.kill();
                panic!("monitor did not exit within 5s — --timeout did not fire");
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("wait failed: {e}"),
        }
    }

    let out = child.wait_with_output().expect("read output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // No matching traffic → no *message* output. (stdout does carry the single
    // ready line `monitor` prints once the subscription is live.)
    let msgs = message_lines(&stdout);
    assert!(
        msgs.is_empty(),
        "expected no messages for a never-matching rule: {msgs:?}"
    );
}

/// Default `monitor` (no `--type`) routes to BecomeMonitor, so it sees method
/// calls too — not just signals. A `busx call` is a `method_call`; the
/// unprivileged signal subscription could never capture one, so catching it
/// proves BecomeMonitor was the default.
///
/// The race that used to make this flaky (call landing before BecomeMonitor
/// took effect) is removed by waiting for the `{"event":"ready"}` line monitor
/// emits once it is actually live on the bus — only then do we trigger the
/// call. The interface/member filter + `--limit-messages 1` make it exit the
/// instant the call is captured.
#[test]
fn monitor_default_captures_method_call() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let mut child = Command::new(cargo_bin!("busx"))
        .args([
            "--json",
            "--address",
            &addr,
            "monitor",
            // No --type: the default (BecomeMonitor) is what's under test.
            "--interface",
            "org.busx.Test",
            "--member",
            "BumpVolume",
            "--limit-messages",
            "1",
            "--timeout",
            "10s",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    // Read stdout line-by-line. The ready event is the real synchronization
    // point that monitor is live on the bus; only once we see it do we fire
    // the method call (from a background thread, so this loop keeps draining
    // until monitor exits via --limit-messages). This removes the fixed-sleep
    // race entirely.
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = BufReader::new(stdout);
    let mut ready = false;
    let mut collected = String::new();
    for line in reader.lines() {
        let line = line.expect("read monitor line");
        if !ready && line.contains("\"event\":\"ready\"") {
            ready = true;
            let addr2 = addr.clone();
            thread::spawn(move || {
                let status = Command::new(cargo_bin!("busx"))
                    .args([
                        "--address",
                        &addr2,
                        "call",
                        "org.busx.Test",
                        "/org/busx/Test",
                        "org.busx.Test",
                        "BumpVolume",
                        "",
                    ])
                    .status()
                    .expect("trigger call");
                assert!(status.success(), "BumpVolume call failed");
            });
        }
        collected.push_str(&line);
        collected.push('\n');
    }
    assert!(ready, "monitor did not emit a ready event");

    let status = child.wait().expect("monitor exit");
    assert!(status.success(), "monitor failed: {status}");

    // Scan every message line for the one we triggered. BecomeMonitor with no
    // filter also forwards bus lifecycle traffic, so other messages may appear
    // around ours — only ours is a method_call to BumpVolume.
    let found = message_lines(&collected).iter().any(|line| {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        v["type"] == "method_call" && v["member"] == "BumpVolume"
    });
    assert!(
        found,
        "default monitor did not capture the BumpVolume method_call:\n{collected}"
    );
}

/// `--type method_call` selects BecomeMonitor (only BecomeMonitor can see method
/// calls) and filters to method calls at the bus. A `busx call` to the fixture's
/// `BumpVolume` is captured as the single matching message.
#[test]
fn monitor_type_method_call_captures_call() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let mut child = Command::new(cargo_bin!("busx"))
        .args([
            "--json",
            "--address",
            &addr,
            "monitor",
            "--type",
            "method_call",
            "--interface",
            "org.busx.Test",
            "--member",
            "BumpVolume",
            "--limit-messages",
            "1",
            "--timeout",
            "10s",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    // Wait for the ready event before firing the call (race-free), then drain
    // until monitor exits via --limit-messages.
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = BufReader::new(stdout);
    let mut ready = false;
    let mut collected = String::new();
    for line in reader.lines() {
        let line = line.expect("read monitor line");
        if !ready && line.contains("\"event\":\"ready\"") {
            ready = true;
            let addr2 = addr.clone();
            thread::spawn(move || {
                let status = Command::new(cargo_bin!("busx"))
                    .args([
                        "--address",
                        &addr2,
                        "call",
                        "org.busx.Test",
                        "/org/busx/Test",
                        "org.busx.Test",
                        "BumpVolume",
                        "",
                    ])
                    .status()
                    .expect("trigger call");
                assert!(status.success(), "BumpVolume call failed");
            });
        }
        collected.push_str(&line);
        collected.push('\n');
    }
    assert!(ready, "monitor did not emit a ready event");

    let status = child.wait().expect("monitor exit");
    assert!(status.success(), "monitor failed: {status}");

    let first: Value =
        serde_json::from_str(first_message_line(&collected)).expect("first message must be JSON");
    assert_eq!(
        first["type"], "method_call",
        "expected a method_call:\n{collected}"
    );
    assert_eq!(
        first["member"], "BumpVolume",
        "expected BumpVolume:\n{collected}"
    );
}
/// `--match` takes a raw D-Bus match rule, parsed directly — a separate branch
/// from the convenience-flag builder. A raw rule pinning `type='signal'` must
/// route through the unprivileged signal subscription (not BecomeMonitor): it
/// captures the PropertiesChanged signal exactly like the convenience-flag
/// path, proving the raw rule's message type is read and routed correctly.
#[test]
fn monitor_match_signal_rule_captures_signal() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();

    let child = Command::new(cargo_bin!("busx"))
        .args([
            "--json",
            "--address",
            &addr,
            "monitor",
            "--match",
            "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
            "--limit-messages",
            "1",
            "--timeout",
            "10s",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn monitor");

    thread::sleep(Duration::from_millis(800));

    let trigger = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "set",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "volume",
            "d",
            "0.25",
        ])
        .status()
        .expect("trigger set");
    assert!(trigger.success(), "set volume call failed");

    let out = child.wait_with_output().expect("monitor exit");
    assert!(out.status.success(), "monitor failed: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first: Value =
        serde_json::from_str(first_message_line(&stdout)).expect("first line must be JSON");
    assert_eq!(
        first["type"], "signal",
        "--match with type='signal' should capture a signal:\n{stdout}"
    );
    assert_eq!(
        first["member"], "PropertiesChanged",
        "expected PropertiesChanged:\n{stdout}"
    );
    assert_eq!(
        first["interface"], "org.freedesktop.DBus.Properties",
        "expected the Properties interface:\n{stdout}"
    );
}
