// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Async bus connection with the session→system fallback, mirroring
//! the blocking `crate::conn::connect`. `--address` > `--system` > session with
//! silent fallback to system (warn on `--verbose`).

use crate::error::{Error, Result};
use zbus::Connection;

pub async fn connect(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
) -> Result<Connection> {
    if let Some(addr) = address {
        return Ok(zbus::connection::Builder::address(addr)?.build().await?);
    }
    if system {
        return Ok(Connection::system().await?);
    }
    match Connection::session().await {
        Ok(c) => Ok(c),
        Err(e) if user => Err(Error::Msg(format!("cannot connect to session bus: {e}"))),
        Err(e) => {
            if verbose {
                eprintln!(
                    "busx: warning: session bus unavailable ({e}); falling back to system bus"
                );
            }
            Ok(Connection::system().await?)
        }
    }
}
