// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx list` — print the names on the bus. Thin wrapper: runs the
//! async core under `block_on`, then renders (TTY-aware table / tab-separated
//! when piped / JSON array).

use crate::dbus;
use crate::error::Result;
use serde_json::{Value as Json, json};

/// Truncate `s` to `cap` display columns, appending `…` when longer.
///
/// Counts characters, not display width. That is correct here because the only
/// non-trivially wide column is NAME, and D-Bus bus names are pure ASCII by
/// spec (a name is dot-separated elements of `[A-Za-z0-9_-]`, with unique
/// names like `:1.0` — see the D-Bus spec "Bus names" section), so one char is
/// always one column. PROCESS (`/proc/<pid>/comm`) is bounded to 15 bytes and
/// likewise ASCII in practice; PID is a number. So a `unicode-width`-aware
/// count would add a dependency for no behavioural difference here.
fn cap_cell(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        let head: String = s.chars().take(cap.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Format `rows` (NAME, PID, PROCESS) into the TTY table fit to `term_w`
/// columns. PROCESS (the trailing column, from `/proc/<pid>/comm`, ≤ 15 bytes)
/// gets a fixed slot, PID its natural width, and NAME the remainder (truncated
/// to it). PROCESS is never padded (trailing column), so the total width is
/// always ≤ `term_w` — a long name never wraps. The truncation/fit is verified
/// end-to-end by running the real binary inside a PTY (see `tests/list.rs`),
/// not by a unit test.
fn render_table(rows: &[[String; 3]], term_w: usize) -> String {
    const PROC_W: usize = 15;
    let pid_w = rows
        .iter()
        .map(|r| r[1].chars().count())
        .max()
        .unwrap_or(0)
        .max("PID".chars().count());
    // Remainder after PID + PROCESS + the two 2-space separators.
    let name_w = term_w.saturating_sub(pid_w + PROC_W + 4);
    let process = "PROCESS";
    let mut out = String::new();
    out.push_str(&format!(
        "{:<nw$}  {:<pw$}  {}\n",
        "NAME",
        "PID",
        process,
        nw = name_w,
        pw = pid_w,
    ));
    for r in rows {
        out.push_str(&format!(
            "{:<nw$}  {:<pw$}  {}\n",
            cap_cell(&r[0], name_w),
            r[1],
            cap_cell(&r[2], PROC_W),
            nw = name_w,
            pw = pid_w,
        ));
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    json: bool,
    no_unique: bool,
    activatable: bool,
) -> Result<()> {
    let names = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::list::list_names(&conn, no_unique, activatable).await
    })?;

    if json {
        let arr: Vec<Json> = names
            .iter()
            .map(|n| json!({ "name": n.name, "pid": n.pid, "process": n.process }))
            .collect();
        crate::out::print_json(&json!(arr));
    } else {
        let rows: Vec<[String; 3]> = names
            .iter()
            .map(|n| {
                [
                    n.name.clone(),
                    n.pid.map(|p| p.to_string()).unwrap_or_default(),
                    n.process.clone().unwrap_or_default(),
                ]
            })
            .collect();
        if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            // Interactive: aligned table fit to the terminal width.
            let term_w = crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .unwrap_or(80);
            print!("{}", render_table(&rows, term_w));
        } else {
            // Piped: tab-separated, no alignment, no truncation (machine-friendly).
            println!("NAME\tPID\tPROCESS");
            for r in &rows {
                println!("{}\t{}\t{}", r[0], r[1], r[2]);
            }
        }
    }
    Ok(())
}
