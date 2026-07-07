// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx tree` — recursively introspect object paths to build a path tree
//! (spec §6).
//!
//! Starting from `/`, each object's introspection XML lists `<node name="..."/>`
//! children. zbus's ObjectServer synthesizes those entries for every registered
//! path below the current one, so descending through them reaches every object
//! a service exposes. The result is a JSON object mapping each service → sorted
//! array of its object paths.

use crate::conn::connect;
use crate::error::Result;
use serde_json::json;
use zbus::blocking::Connection;

/// The interface whose `Introspect` method we call. Every object implements it.
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Recursively collect `path` and every object reachable beneath it into `out`.
///
/// `<node>` children whose `name` is absolute (starts with `/`) are skipped —
/// the DBus spec lets an XML document reference an external subtree that way,
/// but following it would re-root the walk. Repeated names are guarded against
/// by checking membership before recursing, so the walk always terminates even
/// if the introspection data is self-referential.
fn walk(conn: &Connection, service: &str, path: &str, out: &mut Vec<String>) -> Result<()> {
    out.push(path.to_string());

    // The dedicated `IntrospectableProxy` hard-codes `default_path = "/"`, so it
    // can't target an arbitrary object path. The generic `Proxy` carries the
    // real path and exposes `introspect()` (the same call under the hood).
    let proxy = zbus::blocking::Proxy::new(conn, service, path, INTROSPECTABLE)?;
    let xml: String = proxy.introspect()?;

    // zbus introspection XML ships a `<!DOCTYPE node ...>` declaration, so the
    // default `allow_dtd = false` rejects it (see `crate::introspect::parse_xml`).
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&xml, opts)
        .map_err(|e| crate::error::Error::Msg(format!("parse introspection XML: {e}")))?;

    for child in doc.descendants().filter(|n| n.has_tag_name("node")) {
        let Some(name) = child.attribute("name") else { continue };
        if name.starts_with('/') {
            continue;
        }
        let child_path = format!("{}/{}", path.trim_end_matches('/'), name);
        if !out.contains(&child_path) {
            walk(conn, service, &child_path, out)?;
        }
    }
    Ok(())
}

/// Implementation of `busx tree SERVICE`.
///
/// Recursively walks the single given service from `/`, collecting every
/// object path it exposes.
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;
    let mut paths = Vec::new();
    // A service that refuses introspection at `/` just yields an empty tree
    // rather than aborting.
    let _ = walk(&conn, service, "/", &mut paths);
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
