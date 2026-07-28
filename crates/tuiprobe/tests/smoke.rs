// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, KeyModifiers, TuiProbe, encode_key};

#[test]
fn echo_output_is_captured() {
    let mut probe = TuiProbe::new(80, 5).unwrap();
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg("echo hello-world");
    probe.spawn(cmd).unwrap();
    probe.wait_for(|s| s.contains("hello-world")).unwrap();
    assert!(probe.screen_contents().contains("hello-world"));
}

#[test]
fn enter_sends_carriage_return() {
    // Enter must be encoded as a single CR byte (0x0D), not LF. In raw mode
    // crossterm maps `\r` → `KeyCode::Enter` but leaves `\n` as `Ctrl+J`, so a
    // regression here would silently break Enter in every TUI app under test.
    // Assert the bytes `send_key` actually emits (`encode_key`) rather than the
    // child echoing typed text, which would pass whether Enter sent `\r`, `\n`,
    // or nothing at all.
    let bytes = encode_key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(bytes, b"\r");
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
    let result = probe.wait_for(|s| s.contains("never-appears"));
    assert!(result.is_err());
}
