// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

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
    let first_unique = arr.iter().position(is_unique);
    let last_well_known = arr.iter().rposition(|e| !is_unique(e));
    if let (Some(first_unique), Some(last_well_known)) = (first_unique, last_well_known) {
        assert!(
            last_well_known < first_unique,
            "well-known must precede unique:\n{v}"
        );
    }
}

/// Piped (non-TTY) `list` output is tab-separated and untruncated — a long
/// well-known name appears in full. (The TTY truncation is unit-tested in
/// `ops::list::tests`, since it needs a real terminal width.)
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
