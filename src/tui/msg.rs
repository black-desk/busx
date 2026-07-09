// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update` (spec §6).

use crate::dbus::types::{ObjectNode, ServiceInfo};
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
}

/// A side effect `update` requests; the loop performs the IO. Keeps `update` pure.
pub enum Effect {
    FetchServices,
    FetchObjects(String),
    FetchInterfaces(String, String),
    FetchProperties(String, String, String),
}
