// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx get` / `busx set` — thin wrappers over the async core.
//!
//! `get` overloads:
//! - no property names ⇒ `GetAll` (empty interface = all interfaces);
//! - named interface + no names ⇒ `GetAll(interface)`;
//! - named interface + names ⇒ `Get` per name;
//! - empty interface + names ⇒ error (`Get` needs an interface).

use crate::dbus;
use crate::error::{Error, Result};
use serde_json::{Map, Value as Json, json};
use zvariant::OwnedValue;

#[allow(clippy::too_many_arguments)]
pub fn get(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: Option<&str>,
    props: &[String],
) -> Result<()> {
    let get_all_only = props.is_empty();
    match interface {
        // No/empty interface: GetAll over all interfaces is allowed; Get is not.
        None | Some("") if get_all_only => {
            let map = async_global_executor::block_on(async {
                let conn = dbus::conn::connect(user, system, address, verbose).await?;
                dbus::property::get_all(&conn, service, object, "").await
            })?;
            print_map(&map, json)
        }
        None | Some("") => Err(Error::Msg(
            "get: --interface is required when reading individual properties".into(),
        )),
        // Named interface.
        Some(name) => {
            if get_all_only {
                let map = async_global_executor::block_on(async {
                    let conn = dbus::conn::connect(user, system, address, verbose).await?;
                    dbus::property::get_all(&conn, service, object, name).await
                })?;
                print_map(&map, json)
            } else {
                let values = async_global_executor::block_on(async {
                    let conn = dbus::conn::connect(user, system, address, verbose).await?;
                    let mut vs: Vec<OwnedValue> = Vec::with_capacity(props.len());
                    for p in props {
                        vs.push(dbus::property::get_one(&conn, service, object, name, p).await?);
                    }
                    Ok::<_, Error>(vs)
                })?;
                if json {
                    let arr: Vec<_> = values.iter().map(|v| crate::value::decode::to_tagged(v)).collect();
                    crate::out::print_json(&json!(arr));
                } else {
                    for v in &values {
                        println!("{}  {}", v.value_signature(), crate::value::pretty::pretty(v));
                    }
                }
                Ok(())
            }
        }
    }
}

/// Implementation of `busx set`. `signature` is the busctl-style type
/// code of the single value; `value_tokens` are the positional value tokens,
/// both routed through the shared encoder.
#[allow(clippy::too_many_arguments)]
pub fn set(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    service: &str,
    object: &str,
    interface: &str,
    property: &str,
    signature: &str,
    value: &[String],
) -> Result<()> {
    async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::property::set(&conn, service, object, interface, property, signature, value).await
    })
}

/// Print a `GetAll` result keyed by property name (sorted human form, or JSON
/// object).
fn print_map(map: &[(String, OwnedValue)], json: bool) -> Result<()> {
    if json {
        let mut obj = Map::new();
        for (k, v) in map {
            obj.insert(k.clone(), crate::value::decode::to_tagged(v));
        }
        crate::out::print_json(&Json::Object(obj));
    } else {
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in entries {
            println!("{}  {}  {}", k, v.value_signature(), crate::value::pretty::pretty(v));
        }
    }
    Ok(())
}
