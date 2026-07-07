// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx introspect` — fetch introspection XML via
//! `org.freedesktop.DBus.Introspectable.Introspect` and emit it as a JSON
//! array of interface objects (spec §6).

use crate::conn::connect;
use crate::error::Result;
use serde_json::Value as Json;

/// The interface whose `Introspect` method we call. Every object implements it.
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Implementation of `busx introspect SVC OBJ [IFACE]`.
///
/// With no `IFACE`, every interface is emitted. With `IFACE`, only the
/// matching interface is kept (still returned as a one-element-or-empty array,
/// matching the documented shape).
#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: Option<&str>,
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;

    // The dedicated `IntrospectableProxy` hard-codes `default_path = "/"`, so it
    // can't target an arbitrary object path. The generic `Proxy` carries the
    // real path and exposes `introspect()` (the same call under the hood).
    let proxy = zbus::blocking::Proxy::new(&conn, service, object, INTROSPECTABLE)?;
    let xml = proxy.introspect()?;

    let parsed = crate::introspect::parse_xml(&xml);
    let filtered = match (interface, parsed.clone()) {
        // The interface filter applies only to a normal interface array.
        (Some(filter), Json::Array(arr)) => {
            let target = Json::from(filter.to_string());
            Json::Array(arr.into_iter().filter(|i| i.get("name") == Some(&target)).collect())
        }
        (_, other) => other,
    };

    if json {
        crate::out::print_json(&filtered);
    } else {
        print_human(&filtered);
    }
    Ok(())
}

/// Render the parsed introspection as the human form: one block per interface
/// (name on its own line), with methods, properties, and signals listed below.
fn print_human(ifaces: &Json) {
    let Some(arr) = ifaces.as_array() else {
        // The parser emits a `{ "error": ... }` object on XML failure; surface
        // it as-is rather than crashing.
        if let Some(e) = ifaces.get("error").and_then(|v| v.as_str()) {
            eprintln!("busx: {e}");
        }
        return;
    };
    for iface in arr {
        if let Some(name) = iface["name"].as_str() {
            println!("{name}");
        }
        if let Some(methods) = iface["methods"].as_array() {
            for m in methods {
                let mname = m["name"].as_str().unwrap_or("?");
                let in_sig = join_arg_types(&m["in"]);
                let out_sig = join_arg_types(&m["out"]);
                let sig = match (in_sig.is_empty(), out_sig.is_empty()) {
                    (false, false) => format!("{in_sig} → {out_sig}"),
                    (true, false) => format!("→ {out_sig}"),
                    (false, true) => in_sig,
                    (true, true) => String::new(),
                };
                println!("  .{mname:<16} method   {sig}");
            }
        }
        if let Some(signals) = iface["signals"].as_array() {
            for s in signals {
                let sname = s["name"].as_str().unwrap_or("?");
                let args = join_arg_types(&s["args"]);
                println!("  .{sname:<16} signal   {args}");
            }
        }
        if let Some(props) = iface["properties"].as_array() {
            for p in props {
                let pname = p["name"].as_str().unwrap_or("?");
                let ty = p["type"].as_str().unwrap_or("?");
                let access = p["access"].as_str().unwrap_or("");
                println!("  .{pname:<16} prop     {ty} [{access}]");
            }
        }
    }
}

/// Concatenate the `type` fields of an introspection arg array into one
/// signature string (e.g. `["as"]` → `as`, `["s","u"]` → `su`).
fn join_arg_types(args: &Json) -> String {
    args.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["type"].as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}
