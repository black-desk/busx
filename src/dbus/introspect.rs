// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `introspect` — call `Introspect` and parse the XML with `zbus_xml`.
//! `Node::from_reader` yields an owned (`'static`) tree.

use crate::error::{Error, Result};
use zbus_xml::Node;

/// The interface whose `Introspect` method we call. Every object implements it.
pub const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// The standard D-Bus interfaces every object implements (`Properties`,
/// `Introspectable`, `Peer`). They're noise when browsing, so the TUI hides
/// them by default (`--show-standard-interfaces` brings them back).
pub const STANDARD_INTERFACES: &[&str] = &[
    "org.freedesktop.DBus.Properties",
    "org.freedesktop.DBus.Introspectable",
    "org.freedesktop.DBus.Peer",
];

/// Whether `name` is one of the standard D-Bus interfaces every object
/// implements.
pub fn is_standard_interface(name: &str) -> bool {
    STANDARD_INTERFACES.contains(&name)
}

/// Call `org.freedesktop.DBus.Introspectable.Introspect` on `service`/`object`
/// and parse the returned XML into an owned `zbus_xml` tree.
///
/// `zbus_xml`'s parser builds a tree that owns all of its data (it does not
/// borrow the input document), so the result is usable as `Node<'static>`.
pub async fn introspect(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
) -> Result<Node<'static>> {
    let proxy = zbus::Proxy::new(conn, service, object, INTROSPECTABLE).await?;
    let xml: String = proxy
        .call_method("Introspect", &())
        .await?
        .body()
        .deserialize()?;
    // `zbus_xml::Error` has no `From` impl in `crate::error::Error`, so stringify it
    // and carry it via the generic message variant (the tree itself is owned/static).
    Node::from_reader(xml.as_bytes())
        .map_err(|e| Error::Msg(format!("parse introspection XML: {e}")))
}
