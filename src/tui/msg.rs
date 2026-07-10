// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update` (spec §6).

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::state::{ActionResult, ListenTarget};
use crossterm::event::KeyEvent;
use zbus_xml::Node;
use zvariant::OwnedValue;

pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),

    ServicesLoaded(Result<Vec<ServiceInfo>, String>),
    ObjectsLoaded(Result<ObjectNode, String>),
    /// (service, object, the introspection node)
    InterfacesLoaded(String, String, Result<Node<'static>, String>),
    /// (interface name) PropertiesChanged-style refresh result
    PropertiesLoaded(Result<Vec<(String, OwnedValue)>, String>),
    /// A one-shot action (call/get/set) completed.
    ActionResult(Result<ActionResult, String>),
    /// A streaming listen armed its loop; carry the cancel sender so the Result
    /// screen stores it (Esc dropping the screen drops the sender → stop).
    ListenStarted(futures::channel::oneshot::Sender<()>),
    /// One received message from an active streaming listen (a `format_message`
    /// block) — appended to the Result screen's `messages`.
    ListenMessage(String),
}

/// A side effect `update` requests; the loop performs the IO. Keeps `update` pure.
#[derive(Debug)]
pub enum Effect {
    FetchServices,
    FetchObjects(String),
    FetchInterfaces(String, String),
    FetchProperties(String, String, String),
    CallMethod {
        service: String,
        object: String,
        iface: String,
        method: String,
        signature: String,
        args: Vec<String>,
    },
    GetProperty {
        service: String,
        object: String,
        iface: String,
        property: String,
    },
    SetProperty {
        service: String,
        object: String,
        iface: String,
        property: String,
        signature: String,
        value: String,
    },
    /// Start a streaming listen. The loop spawns a task that arms a cancel
    /// channel (`Msg::ListenStarted`) and forwards matching messages
    /// (`Msg::ListenMessage`); signal/property subscribe a `MessageStream`,
    /// method listen is Task 3.
    Listen {
        service: String,
        object: String,
        iface: String,
        target: ListenTarget,
    },
    /// Copy a generated command line to the system clipboard. NOT a dbus op —
    /// `run_effect` does not handle it; only the production `on_effect` closure
    /// in `app::run` does (via `arboard`). The `Effect` seam keeps `arboard`
    /// (which needs a display) out of `update`/`render`/tests.
    CopyToClipboard(String),
}
