// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx list` — print the names on the bus (spec §7). Thin wrapper: runs the
//! async core under `block_on`, then renders (human table ≤80 cols / JSON array).

use crate::dbus;
use crate::error::Result;
use serde_json::{Value as Json, json};

/// Truncate `s` to `cap` display columns, appending `…` when longer.
fn cap_cell(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        let head: String = s.chars().take(cap.saturating_sub(1)).collect();
        format!("{head}…")
    }
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
    let names = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::list::list_names(&conn, unique, acquired, activatable).await
    })?;

    if json {
        let arr: Vec<Json> = names
            .iter()
            .map(|n| json!({ "name": n.name, "pid": n.pid, "process": n.process }))
            .collect();
        crate::out::print_json(&json!(arr));
    } else {
        // NAME  PID  PROCESS, total width ≤ 80. PID ≤ 7 digits, PROCESS (from
        // /proc/<pid>/comm) ≤ 15, so NAME capped at 54.
        const NAME_CAP: usize = 54;
        let mut rows: Vec<[String; 3]> = Vec::with_capacity(names.len());
        for n in &names {
            rows.push([
                cap_cell(&n.name, NAME_CAP),
                n.pid.map(|p| p.to_string()).unwrap_or_default(),
                n.process.clone().unwrap_or_default(),
            ]);
        }
        let cols = ["NAME", "PID", "PROCESS"];
        let mut widths = [
            cols[0].chars().count(),
            cols[1].chars().count(),
            cols[2].chars().count(),
        ];
        for r in &rows {
            for (i, cell) in r.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        println!("{:<w0$}  {:<w1$}  {:<w2$}", cols[0], cols[1], cols[2], w0 = widths[0], w1 = widths[1], w2 = widths[2]);
        for r in &rows {
            println!("{:<w0$}  {:<w1$}  {:<w2$}", r[0], r[1], r[2], w0 = widths[0], w1 = widths[1], w2 = widths[2]);
        }
    }
    Ok(())
}
