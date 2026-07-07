// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::Value;

/// Print compact JSON to stdout (spec §7.2 — never pretty).
pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string(v).expect("json serialize"));
}
