// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, TuiProbe};

#[test]
fn echo_output_is_captured() {
    let mut probe = TuiProbe::new(80, 5).unwrap();
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg("echo hello-world");
    probe.spawn(cmd).unwrap();
    probe.wait_for_text("hello-world").unwrap();
    assert!(probe.screen_contents().contains("hello-world"));
}

#[test]
fn enter_sends_carriage_return() {
    let mut probe = TuiProbe::new(40, 5).unwrap();
    probe.spawn(CommandBuilder::new("cat")).unwrap();

    probe.send_text("abc").unwrap();
    probe.send_key(KeyCode::Enter).unwrap();

    probe.wait_for_text("abc").unwrap();
}

#[test]
fn wait_for_times_out() {
    let mut probe = TuiProbe::builder()
        .cols(40)
        .rows(5)
        .timeout(std::time::Duration::from_millis(200))
        .build()
        .unwrap();
    let mut cmd = CommandBuilder::new("sleep");
    cmd.arg("10");
    probe.spawn(cmd).unwrap();
    let result = probe.wait_for_text("never-appears");
    assert!(result.is_err());
}
