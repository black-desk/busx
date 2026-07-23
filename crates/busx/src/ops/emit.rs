// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx emit` — thin wrapper over the async core. Emits a signal and exits
//! silently on success (errors go to stderr via the caller).

use crate::dbus;
use crate::error::Result;

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    destination: Option<&str>,
    object: &str,
    interface: &str,
    member: &str,
    signature: &str,
    args: &[String],
) -> Result<()> {
    async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::emit::emit_signal(
            &conn, destination, object, interface, member, signature, args,
        )
        .await
    })
}
