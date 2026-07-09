// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). Returns an `Option<Effect>` so it stays
//! IO-free: pushing/loading a screen requests a fetch the loop performs.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{
    tree_items, InterfacesScreen, ObjectsScreen, Screen, ServiceScreen, State,
};

pub fn update(state: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Key(k) => update_key(state, k),
        Msg::Resize(_, _) => None, // handled implicitly by the next draw
        Msg::ServicesLoaded(res) => {
            if let Screen::Service(s) = state.top_mut() {
                load_services(s, res);
            }
            None
        }
        Msg::ObjectsLoaded(res) => load_objects(state, res),
        Msg::InterfacesLoaded(_, _, _) => None, // Task 3
        Msg::PropertiesLoaded(_) => None,       // Task 4
    }
}

fn update_key(state: &mut State, k: KeyEvent) -> Option<Effect> {
    if k.kind != KeyEventKind::Press && !matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
        return None;
    }
    if matches!(k.code, KeyCode::Char('q')) {
        state.quit = true;
        return None;
    }
    if matches!(k.code, KeyCode::Esc) {
        // Esc pops the stack (back); at the root Service screen, Esc quits.
        if state.screens.len() > 1 {
            state.screens.pop();
        } else {
            state.quit = true;
        }
        return None;
    }
    // Service `Enter` → push an Objects screen (loading) + request the tree fetch.
    if k.code == KeyCode::Enter {
        if let Screen::Service(s) = state.top() {
            if let Some(sv) = s.services.get(s.selected) {
                let svc = sv.name.clone();
                state.screens.push(Screen::Objects(ObjectsScreen {
                    service: svc.clone(),
                    tree: ObjectNode { path: "/".into(), children: vec![] },
                    items: vec![],
                    state: Default::default(),
                    loading: true,
                    error: None,
                }));
                return Some(Effect::FetchObjects(svc));
            }
        }
        return None;
    }
    // Per-screen navigation.
    match state.top_mut() {
        Screen::Service(s) => update_service_key(s, k.code),
        Screen::Objects(o) => update_objects_key(o, k.code),
        Screen::Interfaces(_) => {} // Task 3
        Screen::Interface(_) => {}  // Task 4
    }
    None
}

fn update_service_key(s: &mut ServiceScreen, code: KeyCode) {
    match code {
        KeyCode::Down | KeyCode::Char('j') if !s.services.is_empty() => {
            s.selected = (s.selected + 1).min(s.services.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if !s.services.is_empty() => {
            s.selected = s.selected.saturating_sub(1);
        }
        _ => {}
    }
}

fn update_objects_key(o: &mut ObjectsScreen, code: KeyCode) {
    // `Enter` on a tree node → drill into Interfaces (Task 3).
    let mut state = o.state.borrow_mut();
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.key_down();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.key_up();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.key_right();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.key_left();
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

/// Populate the top Objects screen from the tree result; auto-skip if it has
/// exactly one top-level object (drill straight into Interfaces for it).
fn load_objects(state: &mut State, res: Result<ObjectNode, String>) -> Option<Effect> {
    let mut drill = None;
    if let Screen::Objects(o) = state.top_mut() {
        o.loading = false;
        match res {
            Ok(root) => {
                o.items = tree_items(&root);
                if root.children.len() == 1 {
                    drill = Some((o.service.clone(), root.children[0].path.clone()));
                }
                o.tree = root;
            }
            Err(e) => o.error = Some(e),
        }
    }
    if let Some((svc, path)) = drill {
        push_interfaces(state, svc.clone(), path.clone());
        return Some(Effect::FetchInterfaces(svc, path));
    }
    None
}

/// Push an Interfaces screen for (service, object) in loading state.
fn push_interfaces(state: &mut State, service: String, object: String) {
    state.screens.push(Screen::Interfaces(InterfacesScreen {
        service,
        object,
        names: vec![],
        selected: 0,
        loading: true,
        error: None,
    }));
}
