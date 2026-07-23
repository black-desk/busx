// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx tree SERVICE` — recursively introspect a service's object paths and
//! render the tree (indented human form, or nested JSON). The recursive walk
//! itself lives in [`crate::dbus::tree`]; this op only connects + renders.

use crate::dbus;
use crate::dbus::types::ObjectNode;
use crate::error::Result;
use serde_json::{Value as Json, json};

pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    json: bool,
    service: &str,
) -> Result<()> {
    let root = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::tree::object_tree(&conn, service).await
    })?;
    if json {
        crate::out::print_json(&node_to_json(&root));
    } else {
        let mut out = String::new();
        render_node(&root, 0, &mut out);
        print!("{out}");
    }
    Ok(())
}

/// Recursively render a node as the nested JSON shape
/// `{ path, interfaces, children }`.
fn node_to_json(n: &ObjectNode) -> Json {
    json!({
        "path": n.path,
        "interfaces": n.interfaces,
        "children": n.children.iter().map(node_to_json).collect::<Vec<_>>(),
    })
}

/// Recursively render a node indented by `depth`; one line per path.
fn render_node(n: &ObjectNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    out.push_str(&format!("{}{} ({} interfaces)\n", indent, n.path, n.interfaces));
    for child in &n.children {
        render_node(child, depth + 1, out);
    }
}
