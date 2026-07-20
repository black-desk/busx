// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
