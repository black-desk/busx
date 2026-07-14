// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive TUI. Built on the async `dbus::` core.

pub mod app;
pub mod copy;
pub mod msg;
pub mod render;
pub mod state;
pub mod update;

pub use app::run;
pub use copy::{CopyOp, Tool, generate};
pub use msg::{Effect, Msg};
pub use render::render;
pub use state::{ClickTarget, CopyAsPopup, Screen, ServiceScreen, State, flatten_paths};
pub use update::update;
