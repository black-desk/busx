// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `object_tree` — recursively introspect a service's object paths.

use crate::dbus::types::ObjectNode;
use crate::error::{Error, Result};
use zbus_xml::Node;

const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

pub async fn object_tree(conn: &zbus::Connection, service: &str) -> Result<ObjectNode> {
    let mut root = ObjectNode {
        path: "/".to_string(),
        interfaces: 0,
        children: vec![],
    };
    let mut visited = std::collections::HashSet::from(["/".to_string()]);
    // Best-effort: a service that refuses introspection (e.g. at `/`) yields the
    // paths gathered so far rather than aborting — matches the prior CLI behaviour.
    let _ = walk(conn, service, &mut root, &mut visited).await;
    Ok(root)
}

async fn walk(
    conn: &zbus::Connection,
    service: &str,
    node: &mut ObjectNode,
    visited: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let proxy = zbus::Proxy::new(conn, service, &node.path[..], INTROSPECTABLE).await?;
    let xml: String = proxy
        .call_method("Introspect", &())
        .await?
        .body()
        .deserialize()?;
    let parsed = Node::from_reader(xml.as_bytes())
        .map_err(|e| Error::Msg(format!("parse introspection XML: {e}")))?;
    // How many interfaces this object exposes. 0 ⇒ a pure container path (exists
    // only to host sub-objects); the flat TUI view filters such paths out.
    node.interfaces = parsed.interfaces().len();
    for child in parsed.nodes() {
        let Some(name) = child.name() else { continue };
        if name.starts_with('/') {
            // External subtree reference — following it would re-root the walk.
            continue;
        }
        let child_path = format!("{}/{}", node.path.trim_end_matches('/'), name);
        // Cycle guard: skip a path we've already visited. Defends against
        // self-referential introspection data that would otherwise loop forever.
        if !visited.insert(child_path.clone()) {
            continue;
        }
        let mut child_node = ObjectNode {
            path: child_path,
            interfaces: 0,
            children: vec![],
        };
        // `Box::pin` is required: a recursive `async fn` call would otherwise grow
        // an infinitely sized future (E0733).
        Box::pin(walk(conn, service, &mut child_node, visited)).await?;
        node.children.push(child_node);
    }
    Ok(())
}
