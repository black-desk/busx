// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx introspect` — thin wrapper over the async core. Renders `zbus_xml::Node`
//! into the SAME JSON/human shapes as before, so e2e output is unchanged.

use crate::dbus;
use crate::error::Result;
use serde_json::{Value as Json, json};
use zbus_xml::{Arg, ArgDirection, Interface};

// Shared zbus_xml → string helpers (also used by the TUI).
use dbus::introspect::{access_str, sig_str};

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
    let node = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::introspect::introspect(&conn, service, object).await
    })?;

    // Optional interface filter: keep only the named interface (still an array).
    let interfaces: Vec<&Interface> = node.interfaces().iter().collect();
    let interfaces: Vec<&Interface> = match interface {
        Some(filter) => interfaces
            .into_iter()
            .filter(|i| i.name().as_ref() == filter)
            .collect(),
        None => interfaces,
    };

    if json {
        let arr: Vec<Json> = interfaces.iter().copied().map(iface_to_json).collect();
        crate::out::print_json(&json!(arr));
    } else {
        print_human(&interfaces);
    }
    Ok(())
}

fn iface_to_json(iface: &Interface) -> Json {
    let methods: Vec<Json> = iface
        .methods()
        .iter()
        .map(|m| {
            json!({
                "name": m.name().to_string(),
                "in": m.args().iter().filter(|a| a.direction() == Some(ArgDirection::In)).map(arg_to_json).collect::<Vec<_>>(),
                "out": m.args().iter().filter(|a| a.direction() == Some(ArgDirection::Out)).map(arg_to_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    let signals: Vec<Json> = iface
        .signals()
        .iter()
        .map(|s| json!({ "name": s.name().to_string(), "args": s.args().iter().map(arg_to_json).collect::<Vec<_>>() }))
        .collect();
    let props: Vec<Json> = iface
        .properties()
        .iter()
        .map(|p| json!({ "name": p.name().to_string(), "type": sig_str(p.ty()), "access": access_str(p.access()) }))
        .collect();
    json!({ "name": iface.name().to_string(), "methods": methods, "signals": signals, "properties": props })
}

fn arg_to_json(a: &Arg) -> Json {
    json!({ "name": a.name(), "type": sig_str(a.ty()) })
}

fn print_human(interfaces: &[&Interface]) {
    for iface in interfaces {
        println!("{}", iface.name());
        for m in iface.methods() {
            let in_sig: String = m
                .args()
                .iter()
                .filter(|a| a.direction() == Some(ArgDirection::In))
                .map(|a| sig_str(a.ty()))
                .collect();
            let out_sig: String = m
                .args()
                .iter()
                .filter(|a| a.direction() == Some(ArgDirection::Out))
                .map(|a| sig_str(a.ty()))
                .collect();
            let sig = match (in_sig.is_empty(), out_sig.is_empty()) {
                (false, false) => format!("{in_sig} → {out_sig}"),
                (true, false) => format!("→ {out_sig}"),
                (false, true) => in_sig,
                (true, true) => String::new(),
            };
            println!("  .{:<16} method   {sig}", m.name());
        }
        for s in iface.signals() {
            let args: String = s.args().iter().map(|a| sig_str(a.ty())).collect();
            println!("  .{:<16} signal   {args}", s.name());
        }
        for p in iface.properties() {
            println!(
                "  .{:<16} prop     {} [{}]",
                p.name(),
                sig_str(p.ty()),
                access_str(p.access())
            );
        }
    }
}
