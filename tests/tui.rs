// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI snapshot tests (spec §13). Drive the pure State/render core, render to a
//! ratatui TestBackend, compare to an insta golden snapshot. No real bus.

use busx::dbus::types::ServiceInfo;
use busx::tui::{render, State};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Render `state` to a `w`×`h` buffer and return its text view for insta.
/// TestBackend's Display is ratatui's readable buffer_view (text only).
fn render_to_string(state: &State, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, state)).unwrap();
    format!("{}", term.backend())
}

fn svc(name: &str, pid: Option<u64>, process: Option<&str>) -> ServiceInfo {
    ServiceInfo { name: name.into(), pid, process: process.map(Into::into) }
}

#[test]
fn service_screen_renders_populated_list() {
    let state = State::service(vec![
        svc("org.busx.Test", Some(1234), Some("dbus-daemon")),
        svc("org.busx.Other", None, None),
    ]);
    insta::assert_snapshot!(render_to_string(&state, 60, 8));
}
