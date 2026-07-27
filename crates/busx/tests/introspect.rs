// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx introspect` dumps the raw `Introspect()` XML verbatim — no parsing,
//! no filtering, no human/JSON rendering. These tests assert properties of
//! that raw XML (not a structured JSON shape), and that the
//! ignored-global-flag warnings fire.

use assert_cmd::Command;

/// A busx will never reach — used to exercise the warning path without needing
/// a live `dbus-daemon` (the warning is emitted before any connection attempt).
const NOPATH: &str = "unix:path=/nonexistent/busx-introspect-test";

fn introspect_xml(addr: &str) -> String {
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .ok()
        .unwrap();
    String::from_utf8(out.stdout).expect("utf8")
}

#[test]
fn introspect_dumps_raw_xml() {
    let bus = testbus::bus_owned();
    let xml = introspect_xml(&bus.address);

    // It is well-formed XML wrapping a <node>.
    assert!(
        xml.contains("<node"),
        "raw introspect output should be XML starting with <node>:\n{xml}"
    );
    // The fixture's own interface is exposed verbatim.
    assert!(
        xml.contains("org.busx.Test"),
        "missing test interface in XML:\n{xml}"
    );
    // zbus exposes Rust snake_case methods as PascalCase.
    assert!(
        xml.contains("BumpVolume"),
        "missing BumpVolume method in XML:\n{xml}"
    );
    assert!(
        xml.contains("volume"),
        "missing volume property in XML:\n{xml}"
    );
    // Standard interfaces are NOT filtered out — the XML is dumped as-is.
    assert!(
        xml.contains("org.freedesktop.DBus.Properties"),
        "raw dump must include standard interfaces:\n{xml}"
    );
}

/// `--json` is meaningless for a raw-XML dump; the CLI warns and ignores it
/// (it never produces JSON).
#[test]
fn introspect_warns_on_ignored_json() {
    // The connection fails (bogus address), so the command exits non-zero;
    // `.assert()` accepts any exit. The warning is emitted before any
    // connection attempt, so it is already on stderr.
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
            "--address",
            NOPATH,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .assert();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("warning: --json is ignored"),
        "should warn --json is ignored:\n{stderr}"
    );
}

/// `--show-standard-interfaces` is meaningless for a raw-XML dump; the CLI
/// warns and ignores it (standard interfaces stay in the output).
#[test]
fn introspect_warns_on_ignored_show_standard_interfaces() {
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--show-standard-interfaces",
            "--address",
            NOPATH,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .assert();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("warning: --show-standard-interfaces is ignored"),
        "should warn --show-standard-interfaces is ignored:\n{stderr}"
    );
}

/// `--log` is ignored by every CLI subcommand (only the TUI writes a log file).
#[test]
fn introspect_warns_on_ignored_log() {
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--log",
            "/tmp/x.log",
            "--address",
            NOPATH,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .assert();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("warning: --log is ignored"),
        "should warn --log is ignored:\n{stderr}"
    );
}
