// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn call_with_string_array_encodes_and_returns() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--json",
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
            "--json",
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
            "--json",
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

/// Human `call` output: one line per return value as `<type>  <pretty value>`.
/// `Join(["a","b","c"])` returns `"a-b-c"`, rendered `s  "a-b-c"`.
#[test]
fn call_human_prints_type_and_pretty_value() {
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("s  \"a-b-c\""),
        "expected `s  \"a-b-c\"` in human output:\n{stdout}"
    );
}

// --- negative encode paths (busctl-style input is validated before the bus) ---
//
// Each of these fails during `value::encode::parse`, before any D-Bus call, so
// the process exits 1 with a `busx:` diagnostic on stderr. They pin the four
// encoder error paths so a regression (e.g. silently truncating/accepting bad
// input) is caught.

/// An unsupported type code in the signature is rejected.
#[test]
fn call_rejects_unknown_type_code() {
    let addr = common::bus().address.clone();
    Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "Join",
            "z",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unsupported type code"));
}

/// Too few value tokens for the declared signature (`as 1` declares one element
/// but provides none).
#[test]
fn call_rejects_too_few_args() {
    let addr = common::bus().address.clone();
    Command::cargo_bin("busx")
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
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not enough arguments"));
}

/// More value tokens than the declared signature consumes (`as 1 a b` declares
/// one element but provides two).
#[test]
fn call_rejects_extra_args() {
    let addr = common::bus().address.clone();
    Command::cargo_bin("busx")
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
            "1",
            "a",
            "b",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("extra argument"));
}

/// A non-numeric element count is rejected.
#[test]
fn call_rejects_bad_element_count() {
    let addr = common::bus().address.clone();
    Command::cargo_bin("busx")
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
            "x",
            "a",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid element count"));
}
