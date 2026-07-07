mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn list_returns_json_array_with_test_service() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "list"])
        .ok()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array of names");
    assert!(
        arr.iter().any(|n| n == "org.busx.Test"),
        "missing test service in list output: {v}"
    );
}
