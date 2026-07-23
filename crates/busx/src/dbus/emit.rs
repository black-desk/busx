// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `emit_signal` — send a D-Bus signal. `destination` `None` broadcasts;
//! `Some(dest)` unicasts. The body is encoded with the shared busctl-style
//! encoder ([`crate::value::encode::parse`]) and wrapped in a `Structure`,
//! mirroring `dbus::call::call_method`'s positional-arg packing.

use crate::error::{Error, Result};
use zbus::names::{InterfaceName, MemberName};
use zvariant::{ObjectPath, StructureBuilder};

pub async fn emit_signal(
    conn: &zbus::Connection,
    destination: Option<&str>,
    object: &str,
    interface: &str,
    member: &str,
    signature: &str,
    args: &[String],
) -> Result<()> {
    let path = ObjectPath::try_from(object)
        .map_err(|e| Error::Msg(format!("invalid object path `{object}`: {e}")))?;
    let iface = InterfaceName::try_from(interface)
        .map_err(|e| Error::Msg(format!("invalid interface `{interface}`: {e}")))?;
    let member = MemberName::try_from(member)
        .map_err(|e| Error::Msg(format!("invalid member `{member}`: {e}")))?;

    let values = crate::value::encode::parse(signature, args)?;
    if values.is_empty() {
        conn.emit_signal(destination, path, iface, member, &())
            .await?;
    } else {
        let mut builder = StructureBuilder::new();
        for v in values {
            builder = builder.append_field(v);
        }
        let body = builder.build()?;
        conn.emit_signal(destination, path, iface, member, &body)
            .await?;
    }
    Ok(())
}
