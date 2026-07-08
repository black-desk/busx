// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `call_method` — encode busctl-style args, invoke via a generic proxy, return
//! the reply values as owned (spec §7). Encoding is shared (`value::encode`).

use crate::error::Result;
use zvariant::{OwnedValue, Structure, StructureBuilder};

pub async fn call_method(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    signature: &str,
    args: &[String],
) -> Result<Vec<OwnedValue>> {
    let proxy = zbus::Proxy::new(conn, service, object, interface).await?;
    let values = crate::value::encode::parse(signature, args)?;

    // Wrap positional args in a Structure (carries the concatenated signature so
    // the peer sees N positional args). Empty-arg case handled separately.
    let reply = if values.is_empty() {
        proxy.call_method(method, &()).await?
    } else {
        let mut builder = StructureBuilder::new();
        for v in values {
            builder = builder.append_field(v);
        }
        proxy.call_method(method, &builder.build()?).await?
    };

    // Deserialize the reply body as a Structure (accepts any signature; one value
    // wraps in a single-field struct) → owned values.
    let body = reply.body();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let structure: Structure = body.deserialize()?;
    Ok(structure
        .fields()
        .iter()
        .map(|f| f.try_to_owned())
        .collect::<std::result::Result<_, _>>()?)
}
