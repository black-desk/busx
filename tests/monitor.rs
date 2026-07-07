mod common;
use assert_cmd::cargo_bin;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// `busx monitor --signals ... --limit-messages 1` must emit one NDJSON line
/// for the `PropertiesChanged` signal triggered by `BumpVolume`.
///
/// This is a concurrent test: the monitor subscribes first, then a second
/// `busx call` mutates the `volume` property (the fixture emits
/// `org.freedesktop.DBus.Properties.PropertiesChanged`), and the monitor's
/// captured stdout must contain the matching line.
#[test]
fn monitor_emits_propertieschanged() {
    let addr = common::bus().address.clone();

    // Start monitor as a subprocess; it exits after 1 matching message
    // (`--limit-messages`). A `--timeout` backstop keeps the test from hanging
    // if the signal is missed (it should never fire in the happy path).
    let child = Command::new(cargo_bin!("busx"))
        .args([
            "--address",
            &addr,
            "monitor",
            "--signals",
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
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "monitor produced no output:\n{stdout}");

    let first: Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|e| panic!("first line is not JSON ({e}):\n{stdout}"));
    assert_eq!(first["type"], "signal", "expected a signal:\n{stdout}");
    assert_eq!(
        first["member"], "PropertiesChanged",
        "expected PropertiesChanged:\n{stdout}"
    );
    assert_eq!(
        first["interface"],
        "org.freedesktop.DBus.Properties",
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
