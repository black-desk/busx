mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn introspect_lists_test_interface() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "introspect", "org.busx.Test", "/org/busx/Test"])
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
    assert_eq!(arr.len(), 1, "filter keeps exactly the requested iface: {v}");
    assert_eq!(arr[0]["name"], "org.busx.Test");
}

#[test]
fn introspect_interface_filter_unknown_is_empty() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
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
