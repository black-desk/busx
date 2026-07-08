// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared async D-Bus core (spec §3). All concrete D-Bus operations live here as
//! `async fn`s returning typed data (no printing). Both the CLI (`ops/`) and the
//! future TUI consume this module.

pub mod call;
pub mod conn;
pub mod introspect;
pub mod list;
pub mod monitor;
pub mod property;
pub mod tree;
pub mod types;
