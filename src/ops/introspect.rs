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
    let out = match (interface, parsed) {
        // The interface filter applies only to a normal interface array.
        (Some(filter), Json::Array(arr)) => {
            let target = Json::from(filter.to_string());
            Json::Array(arr.into_iter().filter(|i| i.get("name") == Some(&target)).collect())
        }
        (_, other) => other,
    };

    crate::out::print_json(&out);
    Ok(())
}
