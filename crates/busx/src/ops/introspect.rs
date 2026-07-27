// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx introspect` — dump the raw `org.freedesktop.DBus.Introspectable.
//! Introspect` XML for an object, verbatim. No parsing, no filtering, no
//! human/JSON rendering: the XML is the output, exactly as the bus returned it.

use crate::dbus;
use crate::error::Result;

pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    service: &str,
    object: &str,
) -> Result<()> {
    let xml = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::introspect::introspect_xml(&conn, service, object).await
    })?;
    print!("{xml}");
    Ok(())
}
