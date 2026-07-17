// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state. A navigation stack of `Screen`s; `render`
//! draws the top screen + a breadcrumb. `update`/`render` read only this.

use crate::dbus::conn::Bus;
use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::copy::{CopyOp, Tool};
use ratatui::layout::Rect;

/// The navigation context: the service / object / interface the user has
/// drilled into. Each field is set as the user descends one level (service at
/// the Objects screen, object at Interfaces, interface at Interface) and
/// cleared on the way back up, so the context always reflects the current
/// screen's depth. This is the single source of truth — screens carry only view
/// state, never these strings (which is what removes the clone-on-push dance in
/// `update` and lets `render`/copy-op read one consistent context).
#[derive(Clone, Debug, Default)]
pub struct NavContext {
    pub service: String,
    pub object: String,
    pub interface: String,
}

pub struct State {
    /// The accumulated service/object/interface of the current drill path.
    pub nav: NavContext,
    /// Navigation stack; the last element is the currently-shown screen.
    /// **Never empty** — the invariant is upheld by [`State::with_screens`]
    /// (which rejects an empty stack) plus [`State::pop_screen`] (which refuses
    /// at the root). Private so external code can't break it; the field is
    /// reached via [`State::top`] / [`State::top_mut`] / [`State::screens`].
    screens: Vec<Screen>,
    pub quit: bool,
    /// The copy-as popup overlay (`Some` while open; `c` opens, `Esc`/`Enter`
    /// close). Rendered on top of the current screen by `render::render_popup`.
    pub popup: Option<CopyAsPopup>,
    /// Interactive widget rects from the last render, for mouse hit-testing.
    /// Populated by the loop after `render` (render writes them to an out-param).
    pub click_targets: Vec<(Rect, ClickTarget)>,
    /// The `?` help overlay (open/closed). Renders on top of the current screen;
    /// captures keys while open (Esc/`?` close it; other keys are swallowed).
    pub help_open: bool,
    /// The bus busx is connected to, so copy-as emits the matching
    /// `--user`/`--system`/`--address` flag per tool. Set once at startup in
    /// `app::run`; defaults to [`Bus::Session`] (the test default).
    pub bus: Bus,
    /// Whether the Interfaces list shows the standard D-Bus interfaces
    /// (Properties/Introspectable/Peer). Hidden by default; set from
    /// `--show-standard-interfaces` at startup.
    pub show_standard_interfaces: bool,
}

/// A clickable region recorded by `render`, mapping a screen rect to what a
/// left-click there should do (so the mouse handler can hit-test).
#[derive(Clone, Debug, PartialEq)]
pub enum ClickTarget {
    ServiceRow(usize),
    ObjectsRow(usize),
    InterfacesRow(usize),
    MethodRow(usize),
    PropertyRow(usize),
    SignalRow(usize),
    ActionButton(usize),
    DetailField(usize),
    DetailTrigger,
    PopupTool(usize),
}

/// The copy-as popup state: the operation being rendered, each tool's generated
/// command (or `None` = unsupported → shown greyed), and the focused tool index.
///
/// `commands` is precomputed once when the popup opens so navigation and preview
/// are pure reads (no `generate` calls on every keypress).
#[derive(Clone, Debug)]
pub struct CopyAsPopup {
    /// The operation being rendered as another tool's command.
    pub op: CopyOp,
    /// Per-tool generated command (`None` = the tool can't express it). Indexed
    /// in [`Tool::ALL`] order so `selected` indexes both this and the popup rows.
    pub commands: [(Tool, Option<String>); 4],
    /// The focused tool row (0..=3), in [`Tool::ALL`] order.
    pub selected: usize,
    /// Status shown after a copy attempt: `None` = no copy yet;
    /// `Some("copying…")` / `Some("copied")` / `Some("error: …")`. Rendered as a
    /// status line at the bottom of the popup. Set by the `Msg::ClipboardResult`
    /// handler (and the transient "copying…" placeholder on Enter).
    pub status: Option<String>,
}

pub enum Screen {
    Service(ServiceScreen),
    Objects(ObjectsScreen),
    Interfaces(InterfacesScreen),
    Interface(InterfaceScreen),
    Detail(DetailScreen),
    Result(ResultScreen),
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
/// rather than collapsed into a tree. The service itself lives in
/// [`State::nav`].
pub struct ObjectsScreen {
    pub paths: Vec<String>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

/// The non-standard interfaces of one object. The service/object live in
/// [`State::nav`].
pub struct InterfacesScreen {
    pub names: Vec<String>,
    /// Cached introspection of this object — the source of `names` now and of the
    /// interface members (methods/properties/signals) when drilling in.
    pub node: Option<zbus_xml::Node<'static>>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

/// One method: `name` + concatenated IN-signature (display) + per-IN-arg
/// (name, signature) for the call Detail form's input fields.
#[derive(Clone, Debug)]
pub struct MethodMember {
    pub name: String,
    pub signature: String,
    pub args: Vec<(String, String)>,
}

/// One interface: methods / properties (with values) / signals, three columns,
/// plus a right-side action-button bar for the focused column's selected member.
/// The service/object/interface live in [`State::nav`].
pub struct InterfaceScreen {
    pub methods: Vec<MethodMember>,
    /// (name, signature, access) per property.
    pub properties: Vec<(String, String, String)>,
    /// (name, signature) per signal.
    pub signals: Vec<(String, String)>,
    /// GetAll snapshot: property name → pretty value. Refreshed on load / `r`.
    pub prop_values: Vec<(String, String)>,
    pub focus: InterfaceFocus,
    /// Whether the focus is in the right-side action-button bar (rather than in
    /// a member column). `focus` is always the current column (the one whose
    /// selected member the buttons act on); `in_buttons` decides whether ↑↓
    /// moves `button_selected` (true) or the column's `selected` (false).
    pub in_buttons: bool,
    /// Which action button is highlighted in the right panel.
    pub button_selected: usize,
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

/// An action form. Call = one input per IN-arg; Set = one input; Get = no inputs.
/// The service/object/interface live in [`State::nav`].
pub struct DetailScreen {
    pub kind: ActionKind,
    /// One `tui-input` per form field (call args / set value). Empty for get /
    /// zero-arg calls.
    pub inputs: Vec<tui_input::Input>,
    /// "name  sig" per input (display label). Empty for get / zero-arg calls.
    pub field_labels: Vec<String>,
    pub field_selected: usize,
    pub focus: DetailFocus,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ActionKind {
    Call { method: String, signature: String },
    Get { property: String },
    Set { property: String, signature: String },
    Listen { target: ListenTarget },
}

/// What a listen targets — a signal member, a property's `PropertiesChanged`
/// notifications, or a method-call stream via BecomeMonitor.
#[derive(Clone, Debug)]
pub enum ListenTarget {
    Signal { member: String },
    Property { property: String },
    Method { member: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DetailFocus {
    #[default]
    Field,
    Trigger,
}

/// The outcome of a one-shot action (call/get/set), or a streaming listen.
pub struct ResultScreen {
    pub title: String,
    pub result: Option<ActionResult>,
    pub error: Option<String>,
    pub loading: bool,
    pub scroll: usize,
    /// Streaming-listen mode: appended message blocks (`format_message` output).
    pub messages: Vec<String>,
    /// Cancel sender for an active listen. Stored when `Msg::ListenStarted`
    /// arrives; dropped when this screen is popped (Esc) → the listen task exits.
    pub cancel: Option<futures::channel::oneshot::Sender<()>>,
    /// The operation that produced this Result, so `c` can open a copy-as popup
    /// for it. `None` for Results created without one (test literals, etc.).
    pub op: Option<CopyOp>,
}

#[derive(Clone, Debug)]
pub enum ActionResult {
    Call(Vec<String>), // each reply value, pretty-printed
    Get(String),       // the property value, pretty-printed
    Set,               // success
}

impl State {
    /// A Service screen in the loading state (the TUI's initial screen).
    pub fn loading_service() -> Self {
        State {
            nav: NavContext::default(),
            screens: vec![Screen::Service(ServiceScreen {
                services: vec![],
                selected: 0,
                loading: true,
                error: None,
            })],
            quit: false,
            popup: None,
            click_targets: Vec::new(),
            help_open: false,
            bus: Bus::Session,
            show_standard_interfaces: false,
        }
    }

    /// Build a State with a single populated Service screen (tests / default).
    pub fn service(services: Vec<ServiceInfo>) -> Self {
        State {
            nav: NavContext::default(),
            screens: vec![Screen::Service(ServiceScreen {
                services,
                selected: 0,
                loading: false,
                error: None,
            })],
            quit: false,
            popup: None,
            click_targets: Vec::new(),
            help_open: false,
            bus: Bus::Session,
            show_standard_interfaces: false,
        }
    }

    /// Build a State with a pre-built screen stack and nav context. The stack
    /// must be non-empty (the last element is shown); this is the constructor
    /// tests use in place of a struct literal now that `screens` is private.
    /// Panics on an empty stack to make the never-empty invariant unmissable.
    pub fn with_screens(nav: NavContext, screens: Vec<Screen>) -> Self {
        assert!(
            !screens.is_empty(),
            "screen stack must have at least one screen"
        );
        State {
            nav,
            screens,
            quit: false,
            popup: None,
            click_targets: Vec::new(),
            help_open: false,
            bus: Bus::Session,
            show_standard_interfaces: false,
        }
    }

    /// The currently-shown screen. The stack is never empty (private field,
    /// `with_screens` rejects empty, `pop_screen` refuses at the root), so
    /// `len() - 1` is a valid index — no `expect`/`unwrap` needed.
    pub fn top(&self) -> &Screen {
        &self.screens[self.screens.len() - 1]
    }

    pub fn top_mut(&mut self) -> &mut Screen {
        let last = self.screens.len() - 1;
        &mut self.screens[last]
    }

    /// Read-only view of the screen stack (for depth assertions in tests).
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    /// Push a screen onto the navigation stack.
    pub fn push_screen(&mut self, screen: Screen) {
        self.screens.push(screen);
    }

    /// Pop the top screen, clearing the nav field it owns (service at Objects,
    /// object at Interfaces, interface at Interface). Detail/Result own no nav
    /// field (they sit above Interface). Returns `false` at the root screen,
    /// where there is nothing to pop — the caller then quits instead.
    pub fn pop_screen(&mut self) -> bool {
        if self.screens.len() <= 1 {
            return false;
        }
        match self.screens.last() {
            Some(Screen::Objects(_)) => self.nav.service.clear(),
            Some(Screen::Interfaces(_)) => self.nav.object.clear(),
            Some(Screen::Interface(_)) => self.nav.interface.clear(),
            _ => {}
        }
        self.screens.pop();
        true
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
/// (root first), e.g. `["/org/foo", "/bar"]` — the d-feet flat view. Pure
/// container paths (no interfaces ⇒ no object of their own) are skipped.
pub fn flatten_paths(root: &ObjectNode) -> Vec<String> {
    let mut out = Vec::new();
    if root.interfaces > 0 {
        out.push(root.path.clone());
    }
    for child in &root.children {
        out.extend(flatten_paths(child));
    }
    out
}
