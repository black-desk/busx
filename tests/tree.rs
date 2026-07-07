mod common;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn tree_of_test_service_lists_paths() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "tree", "org.busx.Test"])
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
