// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! busx library root: the modules the `busx` binary and integration tests share
//! (the async dbus core, the tui, and helpers). Not a published library — an
//! internal code/test-sharing surface.

pub mod cli;
pub mod complete;
pub mod dbus;
pub mod error;
pub mod ops;
pub mod out;
pub mod tui;
pub mod value;
