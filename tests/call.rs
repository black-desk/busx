mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn call_with_string_array_encodes_and_returns() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "Join",
            "as",
            "3",
            "a",
            "b",
            "c",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "s", "Join returns a single string: {v}");
    assert_eq!(v[0]["data"], "a-b-c");
}

#[test]
fn call_with_dict_of_variant_encodes() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "TakeHints",
            "a{sv}",
            "1",
            "urgency",
            "y",
            "1",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "u", "TakeHints returns a single u32: {v}");
    assert_eq!(v[0]["data"], 1);
}

/// Nesting sanity: an array of variants (`av`) with mixed inner types, fed to
/// `Join`-like paths is not exposed by the fixture, but a variant wrapping a
/// struct round-trips through encode + decode. Use `Join` indirectly is not
/// possible, so this instead exercises `take_hints` with a multi-entry dict to
/// prove the count encoding matches.
#[test]
fn call_with_multi_entry_dict_encodes() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "TakeHints",
            "a{sv}",
            "2",
            "urgency",
            "y",
            "1",
            "category",
            "s",
            "im",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "u");
    assert_eq!(v[0]["data"], 2, "two entries should be counted: {v}");
}
