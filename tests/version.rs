use assert_cmd::Command;

#[test]
fn version_flag_works() {
    Command::cargo_bin("busx")
        .unwrap()
        .args(["--version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("busx "));
}
