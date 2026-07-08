// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update` (spec §6). Keys arrive from crossterm; data results
//! arrive from the async `dbus::` workers over the flume channel.

use crate::dbus::types::ServiceInfo;
use crossterm::event::KeyEvent;

pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    ServicesLoaded(Result<Vec<ServiceInfo>, String>),
}
