// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). No IO.

use crossterm::event::{KeyCode, KeyEventKind};

use crate::dbus::types::ServiceInfo;
use crate::tui::msg::Msg;
use crate::tui::state::{Screen, ServiceScreen, State};

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Key(k) => update_key(state, k),
        Msg::Resize(_, _) => {} // handled implicitly: the next draw reads frame.area()
        Msg::ServicesLoaded(res) => {
            if let Screen::Service(s) = state.top_mut() {
                load_services(s, res);
            }
        }
        // Tasks 2–4 fill these in.
        Msg::ObjectsLoaded(_) => {}
        Msg::InterfacesLoaded(_, _, _) => {}
        Msg::PropertiesLoaded(_) => {}
    }
}

fn update_key(state: &mut State, k: crossterm::event::KeyEvent) {
    if k.kind != KeyEventKind::Press && !matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
        return;
    }
    if matches!(k.code, KeyCode::Char('q')) {
        state.quit = true;
        return;
    }
    if matches!(k.code, KeyCode::Esc) {
        // Esc pops the stack (back); at the root Service screen, Esc quits.
        if state.screens.len() > 1 {
            state.screens.pop();
        } else {
            state.quit = true;
        }
        return;
    }
    match state.top_mut() {
        Screen::Service(s) => update_service_key(s, k.code),
        Screen::Objects(_) => {}    // Task 2
        Screen::Interfaces(_) => {} // Task 3
        Screen::Interface(_) => {}  // Task 4
    }
}

fn update_service_key(s: &mut ServiceScreen, code: KeyCode) {
    match code {
        // Enter on a service is wired in Task 2 (needs the async fetch).
        KeyCode::Down | KeyCode::Char('j') if !s.services.is_empty() => {
            s.selected = (s.selected + 1).min(s.services.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if !s.services.is_empty() => {
            s.selected = s.selected.saturating_sub(1);
        }
        _ => {}
    }
}

fn load_services(s: &mut ServiceScreen, res: Result<Vec<ServiceInfo>, String>) {
    s.loading = false;
    match res {
        Ok(services) => {
            s.selected = s.selected.min(services.len().saturating_sub(1));
            s.services = services;
        }
        Err(e) => s.error = Some(e),
    }
}
