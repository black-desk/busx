// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update`.
//!
//! Bus failures travel as the typed [`crate::error::Error`], not a flattened
//! `String`, so `render` can walk the full cause chain (and future code can
//! branch on the variant). Only [`Msg::ClipboardResult`] keeps a `String`:
//! clipboard failures are ad-hoc (no `Error` variant) and the popup status
//! logic inspects their text.

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::state::{ActionResult, ListenTarget};
use crossterm::event::KeyEvent;
use zbus_xml::Node;
use zvariant::OwnedValue;

pub enum Msg {
    Key(KeyEvent),
    Resize,
    /// A crossterm mouse event (forwarded raw; hit-testing happens in `update`).
    Mouse(crossterm::event::MouseEvent),

    ServicesLoaded(Result<Vec<ServiceInfo>, crate::error::Error>),
    ObjectsLoaded(Result<ObjectNode, crate::error::Error>),
    /// (service, object, the introspection node)
    InterfacesLoaded(String, String, Result<Node<'static>, crate::error::Error>),
    /// Properties refresh result (fetched via GetAll).
    PropertiesLoaded(Result<Vec<(String, OwnedValue)>, crate::error::Error>),
    /// An action completed — a one-shot (call/get/set) result, or a
    /// listen-arming failure (connect error, BecomeMonitor refused, bad match rule).
    ActionResult(Result<ActionResult, crate::error::Error>),
    /// A streaming listen armed its loop; carry the cancel sender so the Result
    /// screen stores it (Esc dropping the screen drops the sender → stop).
    ListenStarted(futures::channel::oneshot::Sender<()>),
    /// One received message from an active streaming listen (a `format_message`
    /// block) — appended to the Result screen's `messages`.
    ListenMessage(String),
    /// A clipboard copy completed (`Ok` = copied, `Err` = why it failed). Surfaced
    /// in the copy-as popup's status line, never printed to the TTY.
    ClipboardResult(std::result::Result<(), String>),
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
    /// method listen uses BecomeMonitor (dedicated connection).
    Listen {
        #[allow(dead_code)]
        service: String,
        object: String,
        iface: String,
        target: ListenTarget,
    },
    /// Copy a generated command line to the system clipboard. Not a D-Bus op,
    /// but dispatched by `run_effect` like the others: it spawns a thread that
    /// tries `wl-copy`/`xclip`/`xsel`, falling back to `arboard`. The `Effect`
    /// seam keeps clipboard IO (which needs a display) out of
    /// `update`/`render`/tests.
    CopyToClipboard(String),
}
