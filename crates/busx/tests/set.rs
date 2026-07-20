// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

#[test]
fn set_then_get_roundtrips() {
    let addr = testbus::bus().address.clone();
    // Set volume to 0.75 (double).
    Command::cargo_bin("busx")
        .unwrap()
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
        .assert()
        .success();
    // Read it back.
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
            "--address",
            &addr,
            "get",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "volume",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("single-get returns an array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["type"], "d");
    assert_eq!(arr[0]["data"], 0.75);
}
