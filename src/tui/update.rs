// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6). Mutates `State` from a `Msg`; performs no IO.

use crossterm::event::{KeyCode, KeyEventKind};

use crate::tui::msg::Msg;
use crate::tui::state::{Screen, ServiceScreen, State};

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Key(k) => {
            // Quit is global.
            if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                state.quit = true;
                return;
            }
            // Ignore key-repeat / release events (some terminals send them).
            if k.kind != KeyEventKind::Press {
                return;
            }
            match &mut state.screen {
                Screen::Service(s) => update_service_key(s, k.code),
            }
        }
        Msg::Resize(_, _) => {}
        Msg::ServicesLoaded(res) => match &mut state.screen {
            Screen::Service(s) => {
                s.loading = false;
                match res {
                    Ok(services) => {
                        s.selected = s.selected.min(services.len().saturating_sub(1));
                        s.services = services;
                    }
                    Err(e) => s.error = Some(e),
                }
            }
        },
    }
}

fn update_service_key(s: &mut ServiceScreen, code: KeyCode) {
    if s.services.is_empty() {
        return;
    }
    let last = s.services.len() - 1;
    match code {
        KeyCode::Down | KeyCode::Char('j') => s.selected = (s.selected + 1).min(last),
        KeyCode::Up | KeyCode::Char('k') => s.selected = s.selected.saturating_sub(1),
        _ => {}
    }
}
