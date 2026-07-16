// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx tree SERVICE` — thin wrapper. Flattens the core's `ObjectNode` into the
//! sorted path list and prints it (human tree / JSON `{service: [paths]}`).

use crate::dbus;
use crate::dbus::types::ObjectNode;
use crate::error::Result;
use serde_json::json;

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

    let mut paths = Vec::new();
    flatten(&root, &mut paths);
    paths.sort();

    if json {
        let mut tree = serde_json::Map::new();
        tree.insert(service.to_string(), json!(paths));
        crate::out::print_json(&json!(tree));
    } else {
        println!("{service}");
        for p in &paths {
            println!("└─ {p}");
        }
    }
    Ok(())
}

fn flatten(node: &ObjectNode, out: &mut Vec<String>) {
    out.push(node.path.clone());
    for c in &node.children {
        flatten(c, out);
    }
}
