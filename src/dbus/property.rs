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
