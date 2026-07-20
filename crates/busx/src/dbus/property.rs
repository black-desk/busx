// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property read/write via `org.freedesktop.DBus.Properties`. Returns
//! owned values so callers (CLI render, the TUI state) can store them.

use crate::error::{Error, Result};
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zvariant::OwnedValue;

pub async fn get_all(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
) -> Result<Vec<(String, OwnedValue)>> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let name = resolve_iface(iface)?;
    let map = proxy.get_all(name).await?;
    let mut out = Vec::with_capacity(map.len());
    for (k, v) in map {
        out.push((k, v.try_to_owned()?));
    }
    Ok(out)
}

pub async fn get_one(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
    prop: &str,
) -> Result<OwnedValue> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let name = resolve_iface(iface)?;
    Ok(proxy.get(name, prop).await?.try_to_owned()?)
}

/// Fallback for services that don't implement `GetAll`: introspect for the
/// property names, then `Get` each one individually. Unreadable properties
/// (write-only, or any that errors) are silently skipped. An empty `iface`
/// means all interfaces.
///
/// Only the TUI uses this — there the interface name always comes from
/// introspection, so a `GetAll` failure means "not implemented" rather than a
/// bad name. The CLI does **not** fall back, so a `GetAll` failure there
/// surfaces as an error (e.g. a typo'd interface isn't silently masked).
pub async fn get_all_by_one(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
) -> Result<Vec<(String, OwnedValue)>> {
    let node = crate::dbus::introspect::introspect(conn, service, object).await?;
    let mut out = Vec::new();
    for interface in node.interfaces() {
        let iname = interface.name();
        if !iface.is_empty() && &*iname != iface {
            continue;
        }
        for prop in interface.properties() {
            let pname = prop.name().to_string();
            if let Ok(v) = get_one(conn, service, object, &iname, &pname).await {
                out.push((pname, v));
            }
        }
    }
    Ok(out)
}

pub async fn set(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
    prop: &str,
    signature: &str,
    value_tokens: &[String],
) -> Result<()> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let mut parsed = crate::value::encode::parse(signature, value_tokens)?;
    let value = parsed
        .pop()
        .ok_or_else(|| Error::Msg("set: missing value".into()))?;
    if !parsed.is_empty() {
        return Err(Error::Msg("set: expected exactly one value".into()));
    }
    let name = resolve_iface(iface)?;
    proxy.set(name, prop, value).await?;
    Ok(())
}

/// Resolve an interface name. The empty string is a special "all interfaces"
/// sentinel for `GetAll` (not a valid `InterfaceName`), injected unchecked to
/// match dbus-send/busctl semantics.
fn resolve_iface(iface: &str) -> Result<InterfaceName<'_>> {
    if iface.is_empty() {
        Ok(InterfaceName::from_str_unchecked(""))
    } else {
        InterfaceName::try_from(iface)
            .map_err(zbus::Error::from)
            .map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `get_all_by_one` is the per-property `Get` fallback the TUI uses when
    /// `GetAll` is unavailable. For a service that *does* implement GetAll it
    /// must return the same property names as GetAll (proving the fallback path
    /// works). The fixture lives in the `testbus` dev-crate.
    #[test]
    fn get_all_by_one_matches_getall() {
        let addr = testbus::bus().address.clone();
        let (by_all, by_one) = async_global_executor::block_on(async {
            let conn = crate::dbus::conn::connect(false, false, Some(&addr))
                .await
                .expect("connect test bus");
            let svc = "org.busx.Test";
            let obj = "/org/busx/Test";
            let iface = "org.busx.Test";
            let all = get_all(&conn, svc, obj, iface).await.expect("get_all");
            let one = get_all_by_one(&conn, svc, obj, iface)
                .await
                .expect("get_all_by_one");
            (all, one)
        });
        let mut a: Vec<&str> = by_all.iter().map(|(k, _)| k.as_str()).collect();
        let mut b: Vec<&str> = by_one.iter().map(|(k, _)| k.as_str()).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "get_all_by_one should match GetAll's property names");
        assert!(
            a.contains(&"volume"),
            "fixture has a `volume` property (test not vacuous): {a:?}"
        );
    }
}
