// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[test]
fn fixture_starts_a_bus() {
    let b = testbus::bus();
    assert!(b.address.starts_with("unix:"), "address was: {}", b.address);
}
