// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display state for the service browser, inline inspector, and tab-local watches.

use std::collections::VecDeque;

use ratatui::layout::Rect;
use tui_input::Input;

use crate::dbus::conn::Bus;
use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::error::Error;
use crate::tui::copy::{CopyOp, Tool};

pub const EVENT_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub service: String,
    pub object: String,
    pub interface: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Services,
    Browser,
    Watch,
    Expanded,
}

/// Window-wide data plus independent browser tabs. Dereferencing State
/// accesses the active tab; background replies explicitly locate their owner.
pub struct State {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub help_open: bool,
    pub help_scroll: usize,
    pub key_prefix: Option<KeyPrefix>,
    pub click_targets: Vec<(Rect, ClickTarget)>,
    pub show_standard_interfaces: bool,
    pub quit: bool,
    next_id: u64,
}

pub struct Tab {
    pub id: u64,
    pub bus: Bus,
    pub connection: zbus::Connection,
    pub services: Vec<ServiceInfo>,
    pub services_request: u64,
    pub services_loading: bool,
    pub services_error: Option<Error>,
    pub bus_picker: Option<BusPicker>,
    pub page: Page,
    pub service_selected: usize,
    pub browser: Option<Browser>,
    pub watch: Option<Watch>,
    pub expanded: Option<Expanded>,
    pub popup: Option<CopyAsPopup>,
    pub filter: Option<Input>,
    pub status: Option<String>,
    pub clipboard_request: Option<u64>,
    pub scroll: [usize; 3],
    pub follow_selection: [bool; 2],
}

impl Tab {
    pub fn new(id: u64, bus: Bus, connection: zbus::Connection) -> Self {
        Self {
            id,
            bus,
            connection,
            services: vec![],
            services_request: 0,
            services_loading: true,
            services_error: None,
            bus_picker: None,
            page: Page::Services,
            service_selected: 0,
            browser: None,
            watch: None,
            expanded: None,
            popup: None,
            filter: None,
            status: None,
            clipboard_request: None,
            scroll: [0; 3],
            follow_selection: [true; 2],
        }
    }

    pub fn bus_name(&self) -> &'static str {
        match self.bus {
            Bus::Session => "Session",
            Bus::System => "System",
            Bus::Other(_) => "Custom",
        }
    }

    fn title_page(&self) -> Page {
        self.expanded
            .as_ref()
            .filter(|_| self.page == Page::Expanded)
            .map_or(self.page, |expanded| expanded.origin)
    }

    fn title_context(&self) -> Option<&Context> {
        if self.title_page() == Page::Watch {
            self.watch.as_ref().map(|watch| &watch.context)
        } else if self.title_page() == Page::Browser {
            self.browser.as_ref().map(|browser| &browser.context)
        } else {
            None
        }
    }

    pub fn stop_watch(&mut self) {
        if let Some(watch) = &mut self.watch {
            watch.cancel = None;
            watch.stopped = true;
            watch.starting = false;
        }
    }
}

impl std::ops::Deref for State {
    type Target = Tab;
    fn deref(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }
}

impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }
}

impl State {
    pub fn new(bus: Bus, connection: zbus::Connection, show_standard_interfaces: bool) -> Self {
        Self {
            tabs: vec![Tab::new(0, bus, connection)],
            active_tab: 0,
            help_open: false,
            help_scroll: 0,
            key_prefix: None,
            click_targets: vec![],
            show_standard_interfaces,
            quit: false,
            next_id: 0,
        }
    }

    /// IDs are window-wide: closing a tab cannot make its late replies
    /// match a subsequently opened tab or subscription.
    pub fn request_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Keep the shortest service suffix that distinguishes open contexts.
    fn service_title(&self, service: &str, member: Option<&str>) -> String {
        if service.starts_with(':') {
            return service.into();
        }
        let parts: Vec<_> = service.split('.').collect();
        for start in (0..parts.len()).rev() {
            let suffix = parts[start..].join(".");
            let ending = format!(".{suffix}");
            let ambiguous = self
                .tabs
                .iter()
                .filter(|tab| match member {
                    Some(member) => {
                        tab.title_page() == Page::Watch
                            && tab
                                .watch
                                .as_ref()
                                .is_some_and(|watch| watch.target.name() == member)
                    }
                    None => tab.title_page() == Page::Browser,
                })
                .filter_map(Tab::title_context)
                .any(|context| {
                    context.service != service
                        && (context.service == suffix || context.service.ends_with(&ending))
                });
            if !ambiguous {
                return format!("{}{suffix}", if start == 0 { "" } else { "…" });
            }
        }
        service.into()
    }

    pub fn tab_titles(&self) -> Vec<String> {
        let titles: Vec<_> = self
            .tabs
            .iter()
            .map(|tab| match tab.title_page() {
                Page::Services => tab.bus_name().into(),
                Page::Watch => tab.watch.as_ref().map_or_else(
                    || "👁️".into(),
                    |watch| {
                        let shared_member = self
                            .tabs
                            .iter()
                            .filter(|other| other.title_page() == Page::Watch)
                            .filter_map(|other| other.watch.as_ref())
                            .any(|other| {
                                other.target.name() == watch.target.name()
                                    && other.context.service != watch.context.service
                            });
                        let source = if shared_member {
                            format!(
                                "{}:",
                                self.service_title(
                                    &watch.context.service,
                                    Some(watch.target.name())
                                )
                                .trim_start_matches('…')
                            )
                        } else {
                            String::new()
                        };
                        format!("👁️ …{source}{}", watch.target.name())
                    },
                ),
                _ => tab.browser.as_ref().map_or_else(
                    || "🔍".into(),
                    |browser| format!("🔍 {}", self.service_title(&browser.context.service, None)),
                ),
            })
            .collect();
        titles
            .iter()
            .enumerate()
            .map(|(index, title)| {
                let tab = &self.tabs[index];
                let shared_bus = tab.title_context().is_some()
                    && titles.iter().enumerate().any(|(other, candidate)| {
                        other != index && candidate == title && self.tabs[other].bus != tab.bus
                    });
                if shared_bus {
                    format!("{title} · {}", tab.bus_name())
                } else {
                    title.clone()
                }
            })
            .collect()
    }

    pub fn new_tab(&mut self) {
        let id = self.request_id();
        self.tabs
            .push(Tab::new(id, self.bus.clone(), self.connection.clone()));
        self.select_tab(self.tabs.len() - 1);
    }

    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.key_prefix = None;
            self.click_targets.clear();
        }
    }

    pub fn close_tab(&mut self) {
        self.close_tab_at(self.active_tab);
    }

    pub fn close_tab_at(&mut self, index: usize) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        tab.stop_watch();
        if self.tabs.len() == 1 {
            self.quit = true;
            return;
        }
        self.tabs.remove(index);
        let active = self.active_tab - usize::from(index < self.active_tab);
        self.select_tab(active.min(self.tabs.len() - 1));
    }

    pub fn query(&self) -> &str {
        self.filter.as_ref().map_or("", Input::value)
    }

    pub fn watch(&self) -> Option<&Watch> {
        self.watch.as_ref()
    }
    pub fn watch_mut(&mut self) -> Option<&mut Watch> {
        self.watch.as_mut()
    }
}

pub struct Browser {
    pub context: Context,
    pub objects: Vec<NavObject>,
    pub nav_selected: usize,
    pub focus: Focus,
    pub tree_request: u64,
    pub interface_request: u64,
    pub properties_request: u64,
    pub tree_loading: bool,
    pub interface_loading: bool,
    pub properties_loading: bool,
    pub error: Option<Error>,
    pub properties_error: Option<Error>,
    pub members: Members,
    pub category: Category,
    pub selected: [usize; 3],
    pub values: Vec<(String, String)>,
    pub editor: Editor,
}

impl Browser {
    pub fn new(service: String, request: u64) -> Self {
        Self {
            context: Context {
                service,
                ..Context::default()
            },
            objects: vec![],
            nav_selected: 0,
            focus: Focus::Navigation,
            tree_request: request,
            interface_request: 0,
            properties_request: 0,
            tree_loading: true,
            interface_loading: false,
            properties_loading: false,
            error: None,
            properties_error: None,
            members: Members::default(),
            category: Category::Properties,
            selected: [0; 3],
            values: vec![],
            editor: Editor::default(),
        }
    }

    pub fn navigation(&self, query: &str) -> Vec<NavRow> {
        let mut rows = vec![];
        let query = query.to_ascii_lowercase();
        let mut hidden_prefix: Option<String> = None;
        for (index, object) in self.objects.iter().enumerate() {
            if query.is_empty()
                && hidden_prefix
                    .as_ref()
                    .is_some_and(|prefix| object.path.starts_with(prefix))
            {
                continue;
            }
            hidden_prefix = None;
            if query.is_empty() && !object.expanded {
                hidden_prefix = Some(format!("{}/", object.path.trim_end_matches('/')));
            }
            let matches_path = object.path.to_ascii_lowercase().contains(&query);
            let interfaces: Vec<_> = object
                .interfaces
                .iter()
                .enumerate()
                .filter(|(_, name)| matches_path || name.to_ascii_lowercase().contains(&query))
                .collect();
            if matches_path || !interfaces.is_empty() {
                rows.push(NavRow::Object(index));
                if object.expanded || !query.is_empty() {
                    rows.extend(
                        interfaces
                            .into_iter()
                            .map(|(i, _)| NavRow::Interface(index, i)),
                    );
                }
            }
        }
        rows
    }

    /// A filtered row can belong to a folded subtree. Reveal its ancestors
    /// before clearing the filter, so selection stays on the opened object.
    pub fn reveal_object(&mut self, index: usize, expand_selected: bool) {
        let path = self.objects[index].path.clone();
        for object in &mut self.objects {
            let ancestor = object.path != path
                && path.starts_with(&format!("{}/", object.path.trim_end_matches('/')));
            if ancestor || (expand_selected && object.path == path) {
                object.expanded = true;
            }
        }
    }

    pub fn visible_members(&self, query: &str) -> Vec<usize> {
        let query = query.to_ascii_lowercase();
        (0..self.member_count())
            .filter(|&index| {
                self.member_name(index)
                    .to_ascii_lowercase()
                    .contains(&query)
            })
            .collect()
    }

    pub fn member_count(&self) -> usize {
        match self.category {
            Category::Properties => self.members.properties.len(),
            Category::Methods => self.members.methods.len(),
            Category::Signals => self.members.signals.len(),
        }
    }

    pub fn member_name(&self, index: usize) -> &str {
        match self.category {
            Category::Properties => self.members.properties.get(index).map(|p| p.name.as_str()),
            Category::Methods => self.members.methods.get(index).map(|m| m.name.as_str()),
            Category::Signals => self.members.signals.get(index).map(|s| s.name.as_str()),
        }
        .unwrap_or("")
    }

    pub fn selected(&self) -> usize {
        self.selected[self.category as usize]
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Selection previews never execute a remote action, including no-arg calls.
    pub fn select_member(&mut self) {
        let fields = match self.category {
            Category::Methods => self
                .members
                .methods
                .get(self.selected())
                .map(|m| {
                    m.args
                        .iter()
                        .map(|(name, signature)| Field {
                            label: format!(
                                "{}  {signature}",
                                if name.is_empty() { "arg" } else { name }
                            ),
                            input: Input::default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Category::Properties => self
                .members
                .properties
                .get(self.selected())
                .filter(|p| p.writable())
                .map(|p| {
                    vec![Field {
                        label: format!("New value  {}", p.signature),
                        input: Input::default(),
                    }]
                })
                .unwrap_or_default(),
            Category::Signals => vec![],
        };
        self.editor = Editor {
            fields,
            ..Editor::default()
        };
    }

    pub fn actions(&self) -> Vec<Action> {
        if self.interface_loading || self.context.interface.is_empty() || self.member_count() == 0 {
            return vec![];
        }
        let mut actions = match self.category {
            Category::Methods => vec![Action::Call, Action::Watch],
            Category::Properties => {
                let Some(property) = self.members.properties.get(self.selected()) else {
                    return vec![];
                };
                let mut actions = vec![];
                if property.writable() {
                    actions.push(Action::Write);
                }
                if property.readable() {
                    actions.push(Action::Read);
                }
                actions.push(Action::Watch);
                actions
            }
            Category::Signals => vec![Action::Watch],
        };
        actions.push(Action::Copy);
        if self.editor.outcome.is_some() {
            actions.push(Action::Expand);
        }
        actions
    }
}

pub struct NavObject {
    pub path: String,
    pub depth: usize,
    pub interfaces: Vec<String>,
    pub expanded: bool,
}

pub fn navigation_objects(root: ObjectNode, show_standard: bool) -> Vec<NavObject> {
    fn walk(node: ObjectNode, show: bool, depth: usize, out: &mut Vec<NavObject>) {
        // Compress unbranched container paths such as /org/gnome. Their full
        // prefix remains in the object's path and the breadcrumb.
        if node.interfaces.is_empty() && node.children.len() == 1 {
            for child in node.children {
                walk(child, show, depth, out);
            }
            return;
        }
        let expanded = !node.children.is_empty() || out.is_empty();
        let mut interfaces: Vec<_> = node
            .interfaces
            .into_iter()
            .filter(|name| show || !crate::dbus::introspect::is_standard_interface(name))
            .collect();
        interfaces.sort();
        out.push(NavObject {
            path: node.path,
            depth,
            interfaces,
            expanded,
        });
        let mut children = node.children;
        children.sort_by(|left, right| left.path.cmp(&right.path));
        for child in children {
            walk(child, show, depth + 1, out);
        }
    }
    let mut objects = vec![];
    walk(root, show_standard, 0, &mut objects);
    objects
}

#[derive(Clone, Copy)]
pub enum KeyPrefix {
    Count(usize),
    G(Option<usize>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavRow {
    Object(usize),
    Interface(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Navigation,
    Members,
    Field(usize),
    Action(usize),
    Result,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Properties,
    Methods,
    Signals,
}

impl Category {
    pub const ALL: [Self; 3] = [Self::Properties, Self::Methods, Self::Signals];
    pub fn name(self) -> &'static str {
        match self {
            Self::Properties => "Properties",
            Self::Methods => "Methods",
            Self::Signals => "Signals",
        }
    }
}

#[derive(Default)]
pub struct Members {
    pub methods: Vec<Method>,
    pub properties: Vec<Property>,
    pub signals: Vec<Signal>,
}

pub struct Method {
    pub name: String,
    pub signature: String,
    pub returns: String,
    pub args: Vec<(String, String)>,
}

pub struct Property {
    pub name: String,
    pub signature: String,
    pub access: String,
}

impl Property {
    pub fn readable(&self) -> bool {
        self.access != "write"
    }
    pub fn writable(&self) -> bool {
        self.access != "read"
    }
    pub fn mode(&self) -> &'static str {
        match self.access.as_str() {
            "read" => "r",
            "write" => "w",
            _ => "rw",
        }
    }
}

pub struct Signal {
    pub name: String,
    pub signature: String,
}

#[derive(Default)]
pub struct Editor {
    pub fields: Vec<Field>,
    pub editing: bool,
    pub pending: Option<u64>,
    pub outcome: Option<Outcome>,
    pub last_op: Option<CopyOp>,
    pub scroll: usize,
    pub hscroll: usize,
}

pub struct Field {
    pub label: String,
    pub input: Input,
}

pub struct Outcome {
    pub result: Option<ActionResult>,
    pub error: Option<Error>,
}

impl Outcome {
    pub fn text(&self) -> String {
        if let Some(error) = &self.error {
            return error_text(error);
        }
        match &self.result {
            Some(ActionResult::Call(values)) if values.is_empty() => {
                "Success (no return values)".into()
            }
            Some(ActionResult::Call(values)) => values.join("\n"),
            Some(ActionResult::Get(value)) => value.clone(),
            Some(ActionResult::Set) => "Written successfully".into(),
            None => String::new(),
        }
    }
}

pub fn error_text(error: &Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(&format!("\n  caused by: {cause}"));
        source = cause.source();
    }
    text
}

pub enum ActionResult {
    Call(Vec<String>),
    Get(String),
    Set,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Call,
    Read,
    Write,
    Watch,
    Copy,
    Expand,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Call => "Call",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::Watch => "Watch",
            Self::Copy => "Copy command",
            Self::Expand => "Expand result",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenTarget {
    Signal(String),
    Property(String),
    Method(String),
}

impl ListenTarget {
    pub fn name(&self) -> &str {
        match self {
            Self::Signal(name) | Self::Property(name) | Self::Method(name) => name,
        }
    }
}

pub struct Watch {
    pub id: u64,
    pub context: Context,
    pub target: ListenTarget,
    pub rule: String,
    pub cancel: Option<futures::channel::oneshot::Sender<()>>,
    pub starting: bool,
    pub stopped: bool,
    pub error: Option<Error>,
    pub events: VecDeque<String>,
    pub total: u64,
    /// Pausing freezes the display, not the subscription; both buffers are bounded.
    pub paused: Option<String>,
    pub scroll: usize,
    pub hscroll: usize,
    pub follow: bool,
}

impl Watch {
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "ERROR"
        } else if self.starting {
            "START"
        } else if self.stopped {
            "STOP"
        } else {
            "RUN"
        }
    }
    pub fn text(&self) -> String {
        if let Some(text) = &self.paused {
            return text.clone();
        }
        self.events.iter().cloned().collect::<Vec<_>>().join("\n\n")
    }
}

pub struct Expanded {
    pub origin: Page,
    pub title: String,
    pub text: String,
    pub op: Option<CopyOp>,
    pub scroll: usize,
    pub hscroll: usize,
}

pub struct CopyAsPopup {
    pub commands: Vec<(Tool, Option<String>)>,
    pub selected: usize,
    pub status: Option<String>,
    pub scroll: usize,
}

pub struct BusPicker {
    pub selected: usize,
    pub address: Input,
    pub pending: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ClickTarget {
    Up { to_navigation: bool },
    BusPicker,
    BusMenu,
    BusDismiss,
    BusOption(usize),
    BusAddress,
    Filter,
    ServicesPane,
    NavigationPane,
    MembersPane,
    WatchEvents,
    ExpandedResult,
    Service(usize),
    Navigation(usize, Focus),
    Category(Category),
    Member(usize),
    Field(usize, u16),
    Action(usize),
    Result,
    Tab(usize),
    CloseTab(usize),
    NewTab,
    WatchAction(usize),
    PopupTool(usize),
    PopupCopy,
}
