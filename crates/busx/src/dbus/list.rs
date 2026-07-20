// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `list_names` — names on the bus + best-effort PID/process.

use crate::dbus::types::ServiceInfo;
use crate::error::Result;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

pub async fn list_names(
    conn: &zbus::Connection,
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<Vec<ServiceInfo>> {
    let dbus = DBusProxy::new(conn).await?;
    let mut names: Vec<String> = if activatable {
        dbus.list_activatable_names().await?
    } else {
        dbus.list_names().await?
    }
    .into_iter()
    .map(|n| n.to_string())
    .collect();

    // `--unique` and `--acquired` are mutually-exclusive filters; both set = no filter.
    if unique && !acquired {
        names.retain(|n| n.starts_with(':'));
    } else if acquired && !unique {
        names.retain(|n| !n.starts_with(':'));
    }
    // Well-known names first (alphabetical), then unique (`:1.x`) names — the
    // meaningful service names lead, the bus-driver plumbing trails.
    names.sort_by(|a, b| {
        a.starts_with(':')
            .cmp(&b.starts_with(':'))
            .then_with(|| a.cmp(b))
    });

    let mut out = Vec::with_capacity(names.len());
    for n in &names {
        out.push(proc_info(&dbus, n).await);
    }
    Ok(out)
}

/// PID via `GetConnectionUnixProcessID`, process via `/proc/<pid>/comm`. Any
/// failure (bus driver has no PID; non-Linux) degrades to `None`s.
async fn proc_info(dbus: &DBusProxy<'_>, name: &str) -> ServiceInfo {
    let empty = ServiceInfo {
        name: name.to_string(),
        pid: None,
        process: None,
    };
    let bus_name = match BusName::try_from(name) {
        Ok(b) => b,
        Err(_) => return empty,
    };
    let pid = match dbus.get_connection_unix_process_id(bus_name).await {
        Ok(p) => p as u64,
        Err(_) => return empty,
    };
    let process = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim_end_matches('\n').to_string());
    ServiceInfo {
        name: name.to_string(),
        pid: Some(pid),
        process,
    }
}
