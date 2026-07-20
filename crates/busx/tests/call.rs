// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;

#[test]
fn call_with_string_array_encodes_and_returns() {
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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
    let addr = testbus::bus().address.clone();
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

/// A method returning an `h` renders the real fd: the JSON path emits a
/// structured object (kind/target/mode) instead of a meaningless integer.
#[test]
fn call_renders_unix_fd() {
    let addr = testbus::bus().address.clone();
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
            "MakeFd",
            "",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "h", "MakeFd returns one fd: {v}");
    assert_eq!(v[0]["data"]["kind"], "char", "fstat of /dev/null: {v}");
    assert_eq!(v[0]["data"]["target"], "/dev/null", "readlink target: {v}");
    assert_eq!(v[0]["data"]["mode"], "ro", "opened O_RDONLY: {v}");
}

/// Human output resolves the fd inline: `h  <fd /dev/null ro>`.
#[test]
fn call_renders_unix_fd_human() {
    let addr = testbus::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "MakeFd",
            "",
        ])
        .ok()
        .unwrap();
    let s = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        s.contains("<fd /dev/null"),
        "human fd render should resolve /dev/null: {s}"
    );
}

/// A pipe read end renders as kind `pipe` with a `pipe:[ino]` target (inode is
/// non-deterministic, asserted by prefix only).
#[test]
fn call_renders_pipe_fd() {
    let addr = testbus::bus().address.clone();
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
            "MakePipeFd",
            "",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "h", "MakePipeFd returns one fd: {v}");
    assert_eq!(v[0]["data"]["kind"], "pipe", "fstat FIFO: {v}");
    let target = v[0]["data"]["target"].as_str().expect("target present");
    assert!(
        target.starts_with("pipe:["),
        "readlink of a pipe end is pipe:[ino]: {target}"
    );
}

/// A byte array (`ay`) renders as a Rust-style bytestring in human output.
#[test]
fn call_renders_ay_bytestring() {
    let addr = testbus::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "MakeBytes",
            "",
        ])
        .ok()
        .unwrap();
    let s = String::from_utf8(out.stdout).expect("utf8");
    assert!(s.contains("b\"hello\""), "ay renders as a bytestring: {s}");
}

/// Non-printable bytes in `ay` are `\xNN`-escaped.
#[test]
fn call_renders_ay_nonprintable() {
    let addr = testbus::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "MakeRawBytes",
            "",
        ])
        .ok()
        .unwrap();
    let s = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        s.contains("b\"\\x00\\xabc\\xff\""),
        "ay non-printable escapes: {s}"
    );
}

/// The JSON path keeps `ay` as a type-tagged array of bytes (unchanged).
#[test]
fn call_ay_json_stays_tagged_bytes() {
    let addr = testbus::bus().address.clone();
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
            "MakeBytes",
            "",
        ])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v[0]["type"], "ay", "ay tags the array: {v}");
    assert_eq!(v[0]["data"][0]["type"], "y", "elements stay tagged bytes");
    assert_eq!(v[0]["data"][0]["data"], 104, "'h' as a bare byte");
}

/// Control characters in a string value are escaped (via `{:?}`), so a `\n`
/// etc. doesn't inject a raw newline into the line-based output.
#[test]
fn call_renders_control_chars_in_string() {
    let addr = testbus::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "MakeControlString",
            "",
        ])
        .ok()
        .unwrap();
    let s = String::from_utf8(out.stdout).expect("utf8");
    // Tab/newline get the named escapes; U+0001 → `\u{1}` (Rust literal style),
    // all inside the quotes — and no raw newline leaks onto a second line.
    assert!(
        s.contains("\"a\\tb\\nc\\u{1}d\""),
        "control chars escaped inside quotes: {s}"
    );
    assert!(
        !s.contains("a\tb"),
        "no raw tab leaked into the output: {s:?}"
    );
}
