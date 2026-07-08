// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx call` — thin wrapper: run the async core, render return values (human
//! `<sig>  <pretty>` per line / type-tagged JSON array). spec §7.

use crate::dbus;
use crate::error::Result;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    signature: &str,
    args: &[String],
) -> Result<()> {
    let fields = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::call::call_method(&conn, service, object, interface, method, signature, args).await
    })?;

    if json {
        let out: Vec<_> = fields.iter().map(|f| crate::value::decode::to_tagged(f)).collect();
        crate::out::print_json(&json!(out));
    } else {
        for f in &fields {
            println!("{}  {}", f.value_signature(), crate::value::pretty::pretty(f));
        }
    }
    Ok(())
}
