// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Request-tagged events keep asynchronous work associated with its target.

use crate::dbus::conn::Bus;
use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::error::Result;
use crate::tui::copy::CopyOp;
use crate::tui::state::{ActionResult, Context, ListenTarget};

pub enum Msg {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize,
    Connected(u64, Result<(zbus::Connection, Bus)>),
    Services(u64, Result<Vec<ServiceInfo>>),
    Objects(u64, Result<ObjectNode>),
    Interface(u64, Result<zbus_xml::Node<'static>>),
    Properties(u64, Result<Vec<(String, zvariant::OwnedValue)>>),
    Action(u64, u64, Context, CopyOp, Result<ActionResult>),
    WatchReady(u64, futures::channel::oneshot::Sender<()>),
    WatchStarted(u64, String),
    WatchMessage(u64, String),
    WatchEnded(u64, Result<()>),
    Clipboard(u64, std::result::Result<(), String>),
}

#[derive(Debug)]
pub enum Effect {
    Connect(u64, Bus),
    Services(u64, u64),
    Objects(u64, u64, String),
    Interface(u64, u64, Context),
    Properties(u64, u64, Context),
    Execute(u64, u64, Context, CopyOp),
    Listen(u64, u64, Context, ListenTarget),
    Clipboard(u64, String),
}
