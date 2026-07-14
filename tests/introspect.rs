// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn introspect_lists_test_interface() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array of interfaces");

    // The fixture's own interface is present alongside the standard ones.
    assert!(
        arr.iter().any(|i| i["name"] == "org.busx.Test"),
        "missing test iface: {v}"
    );

    let iface = arr.iter().find(|i| i["name"] == "org.busx.Test").unwrap();
    // zbus exposes Rust snake_case methods as PascalCase.
    assert!(
        iface["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "BumpVolume"),
        "missing BumpVolume method: {iface}"
    );
    assert!(
        iface["properties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "volume"),
        "missing volume property: {iface}"
    );
}

#[test]
fn introspect_interface_filter_returns_single_match() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("still an array when filtered");
    assert_eq!(
        arr.len(),
        1,
        "filter keeps exactly the requested iface: {v}"
    );
    assert_eq!(arr[0]["name"], "org.busx.Test");
}

#[test]
fn introspect_interface_filter_unknown_is_empty() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
            "org.does.Not.Exist",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("still an array when filtered");
    assert!(arr.is_empty(), "unknown iface filter → empty array: {v}");
}

/// Human introspect output groups members under their interface name, listing
/// methods/properties (and signals) with their signatures.
#[test]
fn introspect_human_lists_interface_members() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
        ])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("org.busx.Test"),
        "missing iface header:\n{stdout}"
    );
    // zbus exposes Rust snake_case methods as PascalCase.
    assert!(
        stdout.contains("BumpVolume"),
        "missing BumpVolume method:\n{stdout}"
    );
    assert!(
        stdout.contains("volume"),
        "missing volume property:\n{stdout}"
    );
    assert!(
        stdout.contains("method"),
        "missing 'method' kind:\n{stdout}"
    );
    assert!(stdout.contains("prop"), "missing 'prop' kind:\n{stdout}");
}
