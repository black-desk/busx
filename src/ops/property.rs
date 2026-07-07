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
            get_all(&proxy, InterfaceName::from_str_unchecked(""))
        }
        None | Some("") => Err(crate::error::Error::Msg(
            "get: --interface is required when reading individual properties".into(),
        )),
        // Named interface.
        Some(name) => {
            let iface = InterfaceName::try_from(name).map_err(zbus::Error::from)?;
            if get_all_only {
                get_all(&proxy, iface)
            } else {
                let mut arr = Vec::with_capacity(props.len());
                for p in props {
                    let v = proxy.get(iface.as_ref(), p)?;
                    arr.push(crate::value::decode::to_tagged(&v));
                }
                crate::out::print_json(&json!(arr));
                Ok(())
            }
        }
    }
}

/// Run `GetAll` and print the result as a type-tagged JSON object keyed by
/// property name.
fn get_all(proxy: &PropertiesProxy<'_>, iface: InterfaceName<'_>) -> Result<()> {
    let map = proxy.get_all(iface)?;
    let mut obj = Map::new();
    for (k, v) in map.iter() {
        // OwnedValue derefs to Value, so `v` is `&Value` after deref.
        obj.insert(k.clone(), crate::value::decode::to_tagged(v));
    }
    crate::out::print_json(&Json::Object(obj));
    Ok(())
}
