// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state (spec §6, §7). A navigation stack of `Screen`s; `render`
//! draws the top screen + a breadcrumb. `update`/`render` read only this.

use crate::dbus::types::{ObjectNode, ServiceInfo};

#[derive(Default)]
pub struct State {
    /// Navigation stack; the last element is the currently-shown screen.
    /// Never empty (the initial Service screen is pushed at construction).
    pub screens: Vec<Screen>,
    pub quit: bool,
}

pub enum Screen {
    Service(ServiceScreen),
    Objects(ObjectsScreen),
    Interfaces(InterfacesScreen),
    Interface(InterfaceScreen),
}

#[derive(Default)]
pub struct ServiceScreen {
    pub services: Vec<ServiceInfo>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

/// The object paths of one service, shown as a flat list (d-feet style): each
/// row is a full path like `/org/freedesktop/DBus` — multi-level paths expanded
/// rather than collapsed into a tree.
pub struct ObjectsScreen {
    pub service: String,
    pub paths: Vec<String>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

/// The non-standard interfaces of one object.
pub struct InterfacesScreen {
    pub service: String,
    pub object: String,
    pub names: Vec<String>,
    /// Cached introspection of this object — the source of `names` now and of the
    /// interface members (methods/properties/signals) when drilling in (Task 4).
    pub node: Option<zbus_xml::Node<'static>>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

/// One interface: methods / properties (with values) / signals, three columns.
pub struct InterfaceScreen {
    pub service: String,
    pub object: String,
    pub interface: String,
    /// (name, signature) per method.
    pub methods: Vec<(String, String)>,
    /// (name, signature, access) per property.
    pub properties: Vec<(String, String, String)>,
    /// (name, signature) per signal.
    pub signals: Vec<(String, String)>,
    /// GetAll snapshot: property name → pretty value. Refreshed on load / `r`.
    pub prop_values: Vec<(String, String)>,
    pub focus: InterfaceFocus,
    pub selected: [usize; 3],
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InterfaceFocus {
    #[default]
    Methods,
    Properties,
    Signals,
}

impl State {
    /// A Service screen in the loading state (the TUI's initial screen).
    pub fn loading_service() -> Self {
        State {
            screens: vec![Screen::Service(ServiceScreen { services: vec![], selected: 0, loading: true, error: None })],
            quit: false,
        }
    }

    /// Build a State with a single populated Service screen (tests / default).
    pub fn service(services: Vec<ServiceInfo>) -> Self {
        State {
            screens: vec![Screen::Service(ServiceScreen { services, selected: 0, loading: false, error: None })],
            quit: false,
        }
    }

    /// The currently-shown screen.
    pub fn top(&self) -> &Screen {
        self.screens.last().expect("screen stack never empty")
    }

    pub fn top_mut(&mut self) -> &mut Screen {
        self.screens.last_mut().expect("screen stack never empty")
    }

    /// The focus of the top Interface screen (test convenience).
    pub fn top_focus(&self) -> InterfaceFocus {
        match self.top() {
            Screen::Interface(i) => i.focus,
            _ => InterfaceFocus::Methods,
        }
    }

    /// The per-column selection of the top Interface screen (test convenience).
    pub fn top_selected(&self) -> [usize; 3] {
        match self.top() {
            Screen::Interface(i) => i.selected,
            _ => [0, 0, 0],
        }
    }
}

/// Flatten the walked object-path tree into a depth-first list of full paths
/// (root first), e.g. `["/", "/org", "/org/freedesktop"]` — the d-feet flat view.
pub fn flatten_paths(root: &ObjectNode) -> Vec<String> {
    let mut out = vec![root.path.clone()];
    for child in &root.children {
        out.extend(flatten_paths(child));
    }
    out
}
