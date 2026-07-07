// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;

#[test]
fn fixture_starts_a_bus() {
    let b = common::bus();
    assert!(b.address.starts_with("unix:"), "address was: {}", b.address);
}
