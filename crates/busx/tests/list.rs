// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::Read;

/// Run `busx list` with stdout attached to a real PTY of the given width, so
/// the TTY-aware table renderer (column-fit + truncation) runs. `assert_cmd`
/// pipes stdout, which selects the machine tab-separated path instead, so the
/// table layout can only be exercised through an actual terminal.
fn list_in_tty(addr: &str, cols: u16, rows: u16) -> String {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");
    let mut cmd = CommandBuilder::new(busx_binary());
    cmd.arg("--address");
    cmd.arg(addr);
    cmd.arg("list");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn busx list");
    // Drop the slave handle so the cloned reader sees EOF once the child exits.
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut out = Vec::new();
    reader.read_to_end(&mut out).expect("read pty output");
    let status = child.wait().expect("wait busx list");
    assert!(status.success(), "busx list failed in pty: {status:?}");
    String::from_utf8_lossy(&out).replace("\r\n", "\n")
}

/// Locate the compiled busx binary (tests run inside `target/debug/deps/`).
fn busx_binary() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("busx")
}

/// `--json list` returns an array of `{ name, pid, process }` objects; the test
/// service must be among them. PIDs are environment-dependent, so only the
/// structure is asserted (a present, optional `pid`).
#[test]
fn list_returns_json_array_with_test_service() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--json", "--address", &addr, "list"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array of {name,pid,process}");
    let test = arr
        .iter()
        .find(|e| e["name"] == "org.busx.Test")
        .unwrap_or_else(|| panic!("missing test service: {v}"));
    // Every entry carries name; pid/process are optional but always present as
    // keys (null when unresolvable).
    assert!(test.get("pid").is_some(), "pid key present: {test}");
    assert!(test.get("process").is_some(), "process key present: {test}");
}

/// Default (human) `list` output is an aligned table with a NAME/PID/PROCESS
/// header and the test service on its own line.
#[test]
fn list_human_shows_table_with_test_service() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "list"])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NAME"), "missing NAME header:\n{stdout}");
    assert!(stdout.contains("PID"), "missing PID header:\n{stdout}");
    assert!(
        stdout.contains("PROCESS"),
        "missing PROCESS header:\n{stdout}"
    );
    assert!(
        stdout.contains("org.busx.Test"),
        "missing test service row:\n{stdout}"
    );
}

/// Well-known names are listed before unique (`:1.x`) names.
#[test]
fn list_orders_well_known_before_unique() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--json", "--address", &addr, "list"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array of {name,pid,process}");
    let is_unique = |e: &Value| e["name"].as_str().unwrap_or("").starts_with(':');
    let first_unique = arr
        .iter()
        .position(is_unique)
        .expect("test bus always has at least one unique name");
    let last_well_known = arr
        .iter()
        .rposition(|e| !is_unique(e))
        .expect("test bus always has well-known names");
    assert!(
        last_well_known < first_unique,
        "well-known names must precede unique names:\n{v}"
    );
}

/// Piped (non-TTY) `list` output is tab-separated and untruncated — a long
/// well-known name appears in full. The TTY truncation is verified by
/// `list_tty_truncates_long_name_to_fit` below, which drives the real binary
/// through a PTY.
#[test]
fn list_piped_is_tab_separated_and_untruncated() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "list"])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().next() == Some("NAME\tPID\tPROCESS"),
        "piped output should be tab-separated with this header:\n{stdout}"
    );
    const LONG: &str = "org.busx.TestServiceNameThatIsIntentionallyVeryLongSoItExceedsTheNameColumnWidthLimitOfFiftyFour";
    assert!(
        stdout.contains(LONG),
        "piped (non-TTY) output must not truncate names:\n{stdout}"
    );
}

/// `--no-unique` hides `:1.x` names, keeping only well-known names.
#[test]
fn list_no_unique_hides_unique_names() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--json", "--address", &addr, "list", "--no-unique"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array");
    assert!(
        arr.iter()
            .all(|e| !e["name"].as_str().unwrap_or("").starts_with(':')),
        "--no-unique must hide all :1.x names: {v}"
    );
    assert!(
        arr.iter().any(|e| e["name"] == "org.busx.Test"),
        "well-known service must remain: {v}"
    );
}

/// When `list` writes to a real terminal, a well-known name longer than the
/// NAME column is truncated with `…`, and no row exceeds the terminal width.
/// This is the TTY path (vs. the piped/tab-separated path above); it runs the
/// real binary inside a PTY of a fixed width instead of a private unit test.
#[test]
fn list_tty_truncates_long_name_to_fit() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let stdout = list_in_tty(&addr, 80, 24);
    const LONG: &str = "org.busx.TestServiceNameThatIsIntentionallyVeryLongSoItExceedsTheNameColumnWidthLimitOfFiftyFour";
    assert!(stdout.contains("NAME"), "table header present:\n{stdout}");
    // The overlong name is clipped, so its full form must be absent.
    assert!(
        !stdout.contains(LONG),
        "long name should be truncated, not printed in full:\n{stdout}"
    );
    // ...and at least one data row carries the truncation marker.
    assert!(
        stdout.lines().skip(1).any(|l| l.contains('…')),
        "expected a truncated name row:\n{stdout}"
    );
    // Every rendered line stays within the terminal width (80 cols).
    for line in stdout.lines() {
        assert!(
            line.chars().count() <= 80,
            "row {} cols wide (> 80): {line}",
            line.chars().count()
        );
    }
}
