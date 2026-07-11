// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive TUI (spec §5–§8). Built on the async `dbus::` core.

pub mod app;
pub mod copy;
pub mod msg;
pub mod render;
pub mod state;
pub mod update;

pub use app::run;
pub use copy::{generate, CopyOp, Tool};
pub use msg::{Effect, Msg};
pub use render::render;
pub use state::{flatten_paths, ClickTarget, CopyAsPopup, Screen, ServiceScreen, State};
pub use update::update;
