//! `busx get` — read D-Bus properties via `org.freedesktop.DBus.Properties`.
//!
//! With no property names, runs `GetAll` (returns an object keyed by property
//! name). With one or more names, runs `Get` per name (returns an array, order
//! preserved). Every value is rendered as type-tagged JSON (spec §7.2).

use crate::conn::connect;
use crate::error::Result;
use serde_json::{Map, Value as Json, json};
use zbus::blocking::fdo::PropertiesProxy;
use zbus::names::InterfaceName;

/// Implementation of `busx get`.
///
/// `--interface` is optional:
/// - absent/empty with no property names → `GetAll("")`, i.e. all interfaces
///   (empty string is not a valid `InterfaceName`, so it is injected unchecked,
///   matching dbus-send/busctl semantics);
/// - absent/empty with one or more property names → error (`Get` needs a name);
/// - present → validated and reused for every call.
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
    let conn = connect(user, system, address, verbose)?;
    let proxy = PropertiesProxy::new(&conn, service, object)?;

    let get_all_only = props.is_empty();
    match interface {
        // No/empty interface: GetAll over all interfaces is allowed; Get is not.
        None | Some("") if get_all_only => {
            get_all(&proxy, InterfaceName::from_str_unchecked(""), json)
        }
        None | Some("") => Err(crate::error::Error::Msg(
            "get: --interface is required when reading individual properties".into(),
        )),
        // Named interface.
        Some(name) => {
            let iface = InterfaceName::try_from(name).map_err(zbus::Error::from)?;
            if get_all_only {
                get_all(&proxy, iface, json)
            } else {
                let mut values: Vec<zvariant::Value<'_>> = Vec::with_capacity(props.len());
                for p in props {
                    let v = proxy.get(iface.as_ref(), p)?;
                    values.push(v.into());
                }
                if json {
                    let arr: Vec<_> =
                        values.iter().map(crate::value::decode::to_tagged).collect();
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

/// Implementation of `busx set`.
///
/// `signature` is the busctl-style type code of the single value; `value_tokens`
/// are the positional value tokens. Both are routed through the shared encoder
/// (which expects the signature as its first token), and the resulting value is
/// written via `org.freedesktop.DBus.Properties.Set`. The peer emits
/// `PropertiesChanged` as a side effect (when the property is annotated
/// `emits_changed_signal`).
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
    value_tokens: &[String],
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;
    let proxy = PropertiesProxy::new(&conn, service, object)?;

    // Build the value via the shared encoder: first token is the signature,
    // the rest are value tokens.
    let mut tokens = vec![signature.to_string()];
    tokens.extend(value_tokens.iter().cloned());
    let mut parsed = crate::value::encode::parse(&tokens)?;
    let value = parsed
        .pop()
        .ok_or_else(|| crate::error::Error::Msg("set: missing value".into()))?;
    if !parsed.is_empty() {
        return Err(crate::error::Error::Msg(
            "set: expected exactly one value".into(),
        ));
    }

    let iface = InterfaceName::try_from(interface).map_err(zbus::Error::from)?;
    proxy.set(iface, property, value)?;
    Ok(())
}

/// Run `GetAll` and print the result as a type-tagged JSON object keyed by
/// property name, or — in human mode — one `<name>  <type>  <pretty>` line per
/// property (sorted by name for stable output).
fn get_all(proxy: &PropertiesProxy<'_>, iface: InterfaceName<'_>, json: bool) -> Result<()> {
    let map = proxy.get_all(iface)?;
    if json {
        let mut obj = Map::new();
        for (k, v) in map.iter() {
            obj.insert(k.clone(), crate::value::decode::to_tagged(v));
        }
        crate::out::print_json(&Json::Object(obj));
    } else {
        let mut names: Vec<&String> = map.keys().collect();
        names.sort();
        for k in names {
            let v = &map[k];
            println!("{}  {}  {}", k, v.value_signature(), crate::value::pretty::pretty(v));
        }
    }
    Ok(())
}
