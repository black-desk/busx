// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

/// `tree` walks the test service's object paths and prints each path flush-left
/// with its interface names indented beneath it. Pure container paths (no
/// interfaces) are omitted, and the standard interfaces are hidden by default.
/// The fixture registers `org.busx.Test` at `/org/busx/Test` and `/org/busx/Test/sub`.
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
    assert!(
        stdout.contains("/org/busx/Test"),
        "missing Test path:\n{stdout}"
    );
    assert!(
        stdout.contains("/org/busx/Test/sub"),
        "missing sub path:\n{stdout}"
    );
    // Interface names now appear indented under their paths.
    assert!(
        stdout.contains("org.busx.Test"),
        "missing interface name:\n{stdout}"
    );
    // Standard interfaces are hidden by default.
    assert!(
        !stdout.contains("org.freedesktop.DBus.Properties"),
        "standard interface should be hidden by default:\n{stdout}"
    );
    // A pure container path like `/org` is never a standalone line.
    assert!(
        !stdout.lines().any(|l| l == "/org"),
        "container path /org should not be printed:\n{stdout}"
    );
}

/// The interface indent grows with tree depth: `/org/busx/Test` is a top-level
/// object (its interface is indented 2 spaces) while `/org/busx/Test/sub` is one
/// level deeper (its interface is indented 4 spaces). Both paths print flush-left.
#[test]
fn tree_indents_interfaces_by_depth() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "tree", "org.busx.Test"])
        .ok()
        .unwrap();
    assert!(out.status.success(), "tree should succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Every printed path is flush-left (no leading whitespace).
    for line in stdout.lines() {
        if line.starts_with('/') {
            assert!(
                !line.starts_with(' '),
                "path should be flush-left: {line:?}"
            );
        }
    }
    // The interface line directly under the top-level path is indented 2 spaces,
    // while the one under the nested sub path is indented 4.
    assert!(
        stdout.lines().any(|l| l == "  org.busx.Test"),
        "top-level interface should be indented 2:\n{stdout}"
    );
    assert!(
        stdout.lines().any(|l| l == "    org.busx.Test"),
        "nested interface should be indented 4:\n{stdout}"
    );
}

/// `--show-standard-interfaces` brings back the standard interfaces that are
/// otherwise hidden.
#[test]
fn tree_show_standard_interfaces() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "--show-standard-interfaces",
            "tree",
            "org.busx.Test",
        ])
        .ok()
        .unwrap();
    assert!(out.status.success(), "tree should succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("org.freedesktop.DBus.Properties"),
        "standard interface should appear with --show-standard-interfaces:\n{stdout}"
    );
}

/// `--json tree` returns a nested object whose `interfaces` field lists the
/// interface names (standard ones hidden by default).
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
            n["children"]
                .as_array()
                .and_then(|cs| cs.iter().find_map(|c| find(c, path)))
        }
    }
    let test = find(&v, "/org/busx/Test").expect("/org/busx/Test node");
    let names: Vec<&str> = test["interfaces"]
        .as_array()
        .expect("interfaces is an array")
        .iter()
        .map(|v| v.as_str().expect("interface name is a string"))
        .collect();
    assert!(
        names.contains(&"org.busx.Test"),
        "Test object exposes the org.busx.Test interface: {test}"
    );
    assert!(
        !names.contains(&"org.freedesktop.DBus.Properties"),
        "standard interfaces hidden from JSON by default: {test}"
    );
}
