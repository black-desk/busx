//! `busx list` — print the names on the bus (spec §7).
//!
//! Default output is a human-friendly, column-aligned table
//! `NAME  PID  PROCESS`; `--json` switches to the type-tagged JSON form, an
//! array of `{ name, pid, process }` objects.
//!
//! Flags:
//! - `--activatable`: list *activatable* names instead of currently-owned ones.
//! - `--unique`: keep only unique (`:`-prefixed) names.
//! - `--acquired`: keep only well-known (non-unique) names.
//!
//! `--unique` and `--acquired` are mutually exclusive filters; if both are
//! given they cancel out and all names are returned (matching the documented
//! "either-or" semantics — both set means no filtering).

use crate::conn::connect;
use crate::error::Result;
use serde_json::{Value as Json, json};
use zbus::blocking::fdo::DBusProxy;
use zbus::names::BusName;

/// Per-name PID + process-name enrichment (best-effort: bus-owned names such as
/// `org.freedesktop.DBus` have no owning process and yield `None`).
struct ProcInfo {
    pid: Option<u64>,
    process: Option<String>,
}

impl ProcInfo {
    fn empty() -> Self {
        Self { pid: None, process: None }
    }
}

/// Look up the PID of `name` via `org.freedesktop.DBus.GetConnectionUnixProcessID`,
/// then read `/proc/<pid>/comm` for the process name. Any failure (the bus
/// driver itself has no PID; non-Linux platforms have no `/proc`) degrades to
/// `ProcInfo::empty()` so a single unresolvable name never breaks the listing.
fn proc_info(dbus: &DBusProxy<'_>, name: &str) -> ProcInfo {
    let bus_name = match BusName::try_from(name) {
        Ok(b) => b,
        Err(_) => return ProcInfo::empty(),
    };
    let pid = match dbus.get_connection_unix_process_id(bus_name) {
        Ok(p) => p as u64,
        Err(_) => return ProcInfo::empty(),
    };
    let process = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim_end_matches('\n').to_string());
    ProcInfo { pid: Some(pid), process }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;
    let dbus = DBusProxy::new(&conn)?;
    let mut names: Vec<String> = if activatable {
        dbus.list_activatable_names()?
    } else {
        dbus.list_names()?
    }
    .into_iter()
    .map(|n| n.to_string())
    .collect();
    if unique && !acquired {
        names.retain(|n| n.starts_with(':'));
    } else if acquired && !unique {
        names.retain(|n| !n.starts_with(':'));
    }
    names.sort();

    if json {
        let arr: Vec<Json> = names
            .iter()
            .map(|n| {
                let info = proc_info(&dbus, n);
                json!({
                    "name": n,
                    "pid": info.pid,
                    "process": info.process,
                })
            })
            .collect();
        crate::out::print_json(&json!(arr));
    } else {
        // Build a fixed-column table: NAME  PID  PROCESS. The three header
        // labels also fix the minimum column width so short lists stay aligned.
        let mut rows: Vec<[String; 3]> = Vec::with_capacity(names.len());
        for n in &names {
            let info = proc_info(&dbus, n);
            rows.push([
                n.clone(),
                info.pid.map(|p| p.to_string()).unwrap_or_default(),
                info.process.unwrap_or_default(),
            ]);
        }
        let cols = ["NAME", "PID", "PROCESS"];
        let mut widths = [cols[0].len(), cols[1].len(), cols[2].len()];
        for r in &rows {
            for (i, cell) in r.iter().enumerate() {
                widths[i] = widths[i].max(cell.len());
            }
        }
        println!(
            "{:<w0$}  {:<w1$}  {:<w2$}",
            cols[0],
            cols[1],
            cols[2],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
        );
        for r in &rows {
            println!(
                "{:<w0$}  {:<w1$}  {:<w2$}",
                r[0],
                r[1],
                r[2],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2],
            );
        }
    }
    Ok(())
}
