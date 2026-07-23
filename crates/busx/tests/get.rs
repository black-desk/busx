// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

#[test]
fn getall_returns_tagged_json() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
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
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");

    assert_eq!(v["volume"]["type"], "d", "volume should be type-tagged f64");
    assert_eq!(v["name"]["type"], "s");
    assert_eq!(v["name"]["data"], "busx-test");

    // Non-string-key dict (a{uu}) MUST render as an array of {key,value} pairs,
    // never an object, and never crash (sd-bus #32904). Keys stay native numbers.
    assert_eq!(v["counts"]["type"], "a{uu}");
    assert!(
        v["counts"]["data"].is_array(),
        "counts must be array-of-pairs: {}",
        v["counts"]
    );
    for entry in v["counts"]["data"].as_array().unwrap() {
        assert!(
            entry.is_object(),
            "each counts entry is a {{key,value}} pair"
        );
        assert_eq!(entry["key"]["type"], "u", "counts key is a native u32");
        assert_eq!(entry["value"]["type"], "u", "counts value is a native u32");
        // Keys must be numbers, not stringified.
        assert!(
            entry["key"]["data"].is_number(),
            "counts key must stay numeric: {}",
            entry["key"]
        );
    }

    // String-key dict-of-variant (a{sv}) renders as a JSON object. Each entry
    // value is a variant: {"type":"v","data":{<inner type-tagged>}}.
    assert_eq!(v["hints"]["type"], "a{sv}");
    assert!(
        v["hints"]["data"].is_object(),
        "hints must be a JSON object: {}",
        v["hints"]
    );
    assert_eq!(v["hints"]["data"]["urgency"]["type"], "v");
    assert_eq!(v["hints"]["data"]["urgency"]["data"]["type"], "y");
    assert_eq!(v["hints"]["data"]["urgency"]["data"]["data"], 1);
}

#[test]
fn get_single_property() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
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
            "name",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("single-get returns an array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["type"], "s");
    assert_eq!(arr[0]["data"], "busx-test");
}

/// Human `get` (single property) prints `<type>  <pretty value>` — e.g.
/// `d  0.5` for the fixture's `volume`.
#[test]
fn get_single_property_human() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("d  0.5"),
        "expected `d  0.5` in human output:\n{stdout}"
    );
}

/// Giving property names but no interface must error (Get needs an interface).
/// The message must NOT mention a nonexistent `--interface` flag — interface is
/// a positional in `get`.
#[test]
fn get_without_interface_errors_clearly() {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    // Note: clap fills the optional `interface` positional before `props`, so
    // `get S O volume` parses `volume` AS the interface (→ invalid-interface
    // error elsewhere, never reaching the no-interface path). To reach the
    // no-interface path we pass an explicit empty interface, leaving `volume`
    // as a property name.
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "get",
            "org.busx.Test",
            "/org/busx/Test",
            "",       // explicit empty interface positional
            "volume", // property name → reaches the no-interface error path
        ])
        .assert() // execute regardless of exit status
        .failure(); // exit 1 is expected here
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.to_lowercase().contains("interface"),
        "should mention interface: {stderr}"
    );
    assert!(
        !stderr.contains("--interface"),
        "must not reference nonexistent --interface flag: {stderr}"
    );
}
