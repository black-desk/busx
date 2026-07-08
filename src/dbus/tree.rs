// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `object_tree` — recursively introspect a service's object paths (spec §6).

use crate::dbus::types::ObjectNode;
use crate::error::{Error, Result};
use zbus_xml::Node;

const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

pub async fn object_tree(conn: &zbus::Connection, service: &str) -> Result<ObjectNode> {
    let mut root = ObjectNode { path: "/".to_string(), children: vec![] };
    let mut visited = std::collections::HashSet::from(["/".to_string()]);
    // Best-effort: a service that refuses introspection (e.g. at `/`) yields the
    // paths gathered so far rather than aborting — matches the prior CLI behaviour.
    let _ = walk(conn, service, "/", &mut root.children, &mut visited).await;
    Ok(root)
}

async fn walk(
    conn: &zbus::Connection,
    service: &str,
    path: &str,
    out: &mut Vec<ObjectNode>,
    visited: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let proxy = zbus::Proxy::new(conn, service, path, INTROSPECTABLE).await?;
    let xml: String = proxy.call_method("Introspect", &()).await?.body().deserialize()?;
    let node = Node::from_reader(xml.as_bytes())
        .map_err(|e| Error::Msg(format!("parse introspection XML: {e}")))?;
    for child in node.nodes() {
        let Some(name) = child.name() else { continue };
        if name.starts_with('/') {
            // External subtree reference — following it would re-root the walk.
            continue;
        }
        let child_path = format!("{}/{}", path.trim_end_matches('/'), name);
        // Cycle guard: skip a path we've already visited. Defends against
        // self-referential introspection data that would otherwise loop forever.
        if !visited.insert(child_path.clone()) {
            continue;
        }
        let mut child_node = ObjectNode { path: child_path.clone(), children: vec![] };
        // Recurse before pushing so the borrow on `out` is released each iteration.
        // `Box::pin` is required: a recursive `async fn` call would otherwise grow an
        // infinitely sized future (E0733).
        Box::pin(walk(conn, service, &child_path, &mut child_node.children, visited)).await?;
        out.push(child_node);
    }
    Ok(())
}
