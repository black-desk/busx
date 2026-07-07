// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn tree_of_test_service_lists_paths() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--json", "--address", &addr, "tree", "org.busx.Test"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let paths = v["org.busx.Test"].as_array().expect("paths array");
    assert!(
        paths.iter().any(|p| p == "/org/busx/Test"),
        "missing /org/busx/Test: {v}"
    );
    assert!(
        paths.iter().any(|p| p == "/org/busx/Test/sub"),
        "missing /org/busx/Test/sub: {v}"
    );
}

/// `tree` now requires exactly one service: no service → usage error.
#[test]
fn tree_requires_a_service() {
    Command::cargo_bin("busx")
        .unwrap()
        .args(["tree"])
        .assert()
        .failure();
}

/// `tree` rejects more than one service.
#[test]
fn tree_rejects_multiple_services() {
    let addr = common::bus().address.clone();
    Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "tree", "org.busx.Test", "org.busx.Test"])
        .assert()
        .failure();
}

/// Human tree output: service name on its own line, each path beneath.
#[test]
fn tree_human_shows_service_and_paths() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "tree", "org.busx.Test"])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("org.busx.Test"),
        "missing service header:\n{stdout}"
    );
    assert!(
        stdout.contains("/org/busx/Test"),
        "missing /org/busx/Test:\n{stdout}"
    );
}
