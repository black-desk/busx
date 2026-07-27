// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx tree SERVICE` — recursively introspect a service's object paths and
//! render the tree (indented human form, or nested JSON). The recursive walk
//! itself lives in [`crate::dbus::tree`]; this op only connects + renders.

use crate::dbus;
use crate::dbus::introspect::is_standard_interface;
use crate::dbus::types::ObjectNode;
use crate::error::Result;
use serde_json::{Value as Json, json};

pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    json: bool,
    show_standard_interfaces: bool,
    service: &str,
) -> Result<()> {
    let root = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::tree::object_tree(&conn, service).await
    })?;
    if json {
        crate::out::print_json(&node_to_json(&root, show_standard_interfaces));
    } else {
        let mut out = String::new();
        render_node(&root, 0, show_standard_interfaces, &mut out);
        print!("{out}");
    }
    Ok(())
}

/// The interface names a node exposes, optionally dropping the standard D-Bus
/// interfaces (Properties/Introspectable/Peer) that every object
/// implements — they are noise when browsing.
fn visible_interfaces(n: &ObjectNode, show_standard: bool) -> Vec<&str> {
    n.interfaces
        .iter()
        .map(String::as_str)
        .filter(|name| show_standard || !is_standard_interface(name))
        .collect()
}

/// Recursively render a node as the nested JSON shape
/// `{ path, interfaces, children }`.
fn node_to_json(n: &ObjectNode, show_standard: bool) -> Json {
    json!({
        "path": n.path,
        "interfaces": visible_interfaces(n, show_standard),
        "children": n
            .children
            .iter()
            .map(|c| node_to_json(c, show_standard))
            .collect::<Vec<_>>(),
    })
}

/// Recursively render a node. Each object path is printed flush-left (no
/// indent) on its own line so it is easy to copy; the interface names it
/// exposes are listed below it, indented by 2 * (depth + 1) spaces so the
/// indentation depth still conveys the tree level. Pure container paths (no
/// interfaces after filtering) are skipped, but their children are still
/// walked — so the depth only advances across rendered nodes.
fn render_node(n: &ObjectNode, depth: usize, show_standard: bool, out: &mut String) {
    let ifaces = visible_interfaces(n, show_standard);
    let rendered = !ifaces.is_empty();
    if rendered {
        out.push_str(&format!("{}\n", n.path));
        let indent = "  ".repeat(depth + 1);
        for iface in &ifaces {
            out.push_str(&format!("{indent}{iface}\n"));
        }
    }
    for child in &n.children {
        render_node(child, depth + rendered as usize, show_standard, out);
    }
}
