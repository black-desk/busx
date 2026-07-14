// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::error::Result;
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;

/// Resolve which bus to connect to.
///
/// - `--address=ADDR` → that bus only.
/// - `--system` → system bus only.
/// - `--user` → session bus only (no fallback).
/// - neither `--user` nor `--system` (default) → try session, on failure fall
///   back to the system bus. The fallback is silent unless `verbose` is set,
///   in which case a warning is printed to stderr.
pub fn connect(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
) -> Result<Connection> {
    if let Some(addr) = address {
        return Ok(Builder::address(addr)?.build()?);
    }
    if system {
        return Ok(Connection::system()?);
    }
    // `--user` or default: try the session bus first.
    match Connection::session() {
        Ok(c) => Ok(c),
        Err(e) if user => Err(crate::error::Error::Msg(format!(
            "cannot connect to session bus: {e}"
        ))),
        Err(e) => {
            if verbose {
                eprintln!(
                    "busx: warning: session bus unavailable ({e}); falling back to system bus"
                );
            }
            Ok(Connection::system()?)
        }
    }
}
