// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive TUI. Built on the async `dbus::` core.

mod app;
mod copy;
mod msg;
mod render;
mod state;
mod update;

#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod snapshot_tests;

pub use app::run;
