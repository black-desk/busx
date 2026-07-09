// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state (spec §6, §7). A navigation stack of `Screen`s; `render`
//! draws the top screen + a breadcrumb. `update`/`render` read only this.

use crate::dbus::types::{ObjectNode, ServiceInfo};
use std::cell::RefCell;
use tui_tree_widget::TreeState;

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

/// The object-path tree of one service. `tree` is the walked `ObjectNode` root;
/// `items` is the `tui-tree-widget` representation built from it. `state` holds
/// the widget's selection/opened set across frames; it is a `RefCell` because
/// `tui-tree-widget` 0.24's `TreeState` is not `Clone` and `render` must mutate
/// it from a `&State` (interior mutability).
pub struct ObjectsScreen {
    pub service: String,
    pub tree: ObjectNode,
    pub items: Vec<tui_tree_widget::TreeItem<'static, String>>,
    pub state: RefCell<TreeState<String>>,
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

/// Build tui-tree-widget items from a walked object-path tree. Each item's
/// identifier is its full path (unique), text is the last path segment.
pub fn tree_items(root: &ObjectNode) -> Vec<tui_tree_widget::TreeItem<'static, String>> {
    root.children.iter().map(child_item).collect()
}

fn child_item(node: &ObjectNode) -> tui_tree_widget::TreeItem<'static, String> {
    let text = node.path.rsplit('/').next().unwrap_or(&node.path).to_string();
    let children: Vec<_> = node.children.iter().map(child_item).collect();
    if children.is_empty() {
        tui_tree_widget::TreeItem::new_leaf(node.path.clone(), text)
    } else {
        // `new` rejects duplicate child ids; our paths are unique, so this can't fail.
        tui_tree_widget::TreeItem::new(node.path.clone(), text, children).unwrap()
    }
}
