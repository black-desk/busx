// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Async bus connection with the session→system fallback — the single source
//! of truth for connecting to the bus. `--address` > `--system` > session with
//! silent fallback to system (warn on `--verbose`).

use crate::error::{Error, Result};
use zbus::Connection;

/// Which well-known bus a connection ended up on. Used by copy-as to emit the
/// matching bus-selection flag for each D-Bus tool. [`Bus::Other`] carries the
/// custom `--address` so the generated command can target it faithfully (every
/// tool has an address flag, though the syntax differs — see `tui::copy`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Bus {
    /// The session (user) bus.
    #[default]
    Session,
    /// The system bus.
    System,
    /// A custom address supplied via `--address`.
    Other(String),
}

pub async fn connect(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
) -> Result<Connection> {
    let (conn, _bus) = connect_with_bus(user, system, address, verbose).await?;
    Ok(conn)
}

/// Like [`connect`] but also reports which bus was actually reached — needed by
/// the TUI's copy-as feature so generated commands target the same bus busx is
/// on (busctl defaults to the system bus; dbus-send/qdbus to the session bus).
pub async fn connect_with_bus(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
) -> Result<(Connection, Bus)> {
    if let Some(addr) = address {
        return Ok((
            zbus::connection::Builder::address(addr)?.build().await?,
            Bus::Other(addr.into()),
        ));
    }
    if system {
        return Ok((Connection::system().await?, Bus::System));
    }
    match Connection::session().await {
        Ok(c) => Ok((c, Bus::Session)),
        Err(e) if user => Err(Error::Msg(format!("cannot connect to session bus: {e}"))),
        Err(e) => {
            if verbose {
                eprintln!(
                    "busx: warning: session bus unavailable ({e}); falling back to system bus"
                );
            }
            Ok((Connection::system().await?, Bus::System))
        }
    }
}
