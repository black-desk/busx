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
/// always ≤ `term_w` — a long name or a CJK process name (3 bytes/char) never
/// wraps. Pure so the truncation/fit can be unit-tested without a real TTY.
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
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<()> {
    let names = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;
        dbus::list::list_names(&conn, unique, acquired, activatable).await
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

#[cfg(test)]
mod tests {
    use super::render_table;

    /// An overlong NAME is truncated with `…` and the row stays within the
    /// terminal width (here 80 cols). PID is 3 (header), PROCESS 15 → NAME gets
    /// 80 − 3 − 15 − 4 = 58 cols.
    #[test]
    fn render_table_truncates_name_to_fit() {
        let rows = [[
            "org.busx.TestServiceNameThatIsIntentionallyVeryLongSoItExceedsTheColumn".to_string(),
            "42".to_string(),
            "a-process".to_string(),
        ]];
        let out = render_table(&rows, 80);
        let data_line = out.lines().nth(1).unwrap(); // skip header
        assert!(data_line.contains('…'), "expected truncation: {data_line}");
        assert!(
            data_line.chars().count() <= 80,
            "row {} cols wide (> 80): {data_line}",
            data_line.chars().count()
        );
    }

    /// A CJK PROCESS name (3 bytes/char) is truncated to the fixed 15-col slot
    /// and never pushes the row past the terminal width.
    #[test]
    fn render_table_wide_process_stays_within_width() {
        let rows = [[
            ":1.5".to_string(),
            "999".to_string(),
            "中文进程名占位".to_string(), // 7 chars, 21 bytes
        ]];
        let out = render_table(&rows, 60);
        for line in out.lines() {
            assert!(
                line.chars().count() <= 60,
                "line {} cols (> 60): {line}",
                line.chars().count()
            );
        }
    }
}
