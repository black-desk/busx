// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

/// `tree` walks the test service's object paths. The fixture registers
/// `/org/busx/Test` and `/org/busx/Test/sub`, both plus the root `/` must
/// appear.
#[test]
fn tree_lists_test_service_paths() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "tree", "org.busx.Test"])
        .ok()
        .unwrap();
    assert!(out.status.success(), "tree should succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("/org/busx/Test"), "missing Test path:\n{stdout}");
    assert!(stdout.contains("/org/busx/Test/sub"), "missing sub path:\n{stdout}");
    assert!(stdout.contains('/'), "root path should appear:\n{stdout}");
}

/// `--json tree` returns a nested `{ path, interfaces, children }` object.
#[test]
fn tree_json_is_nested_object() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--json", "--address", &addr, "tree", "org.busx.Test"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v["path"], "/", "root path: {v}");
    fn find<'a>(n: &'a Value, path: &str) -> Option<&'a Value> {
        if n["path"] == path {
            Some(n)
        } else {
            n["children"].as_array().and_then(|cs| cs.iter().find_map(|c| find(c, path)))
        }
    }
    let test = find(&v, "/org/busx/Test").expect("/org/busx/Test node");
    assert!(
        test["interfaces"].as_u64() >= Some(1),
        "Test object exposes >=1 interface: {test}"
    );
}
