// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI entry point + event loop (spec §5). Real loop lands in Task 5.

use crate::error::Result;

/// Launch the TUI. Temporary stub — replaced by the real loop in Task 5.
pub fn run(_user: bool, _system: bool, _address: Option<&str>, _verbose: bool) -> Result<()> {
    eprintln!("busx: TUI under construction (phase 1)");
    Ok(())
}
