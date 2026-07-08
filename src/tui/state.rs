// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state (spec §6). `render` reads only this — never IO handles.

use crate::dbus::types::ServiceInfo;

#[derive(Default)]
pub struct State {
    pub screen: Screen,
    pub quit: bool,
}

pub enum Screen {
    Service(ServiceScreen),
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Service(ServiceScreen::default())
    }
}

#[derive(Default)]
pub struct ServiceScreen {
    pub services: Vec<ServiceInfo>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl State {
    /// Build a State showing a populated Service screen (for tests / default).
    pub fn service(services: Vec<ServiceInfo>) -> Self {
        State {
            screen: Screen::Service(ServiceScreen {
                services,
                selected: 0,
                loading: false,
                error: None,
            }),
            quit: false,
        }
    }

    /// A Service screen in the loading state (the TUI's initial screen).
    pub fn loading_service() -> Self {
        State {
            screen: Screen::Service(ServiceScreen { services: vec![], selected: 0, loading: true, error: None }),
            quit: false,
        }
    }
}
