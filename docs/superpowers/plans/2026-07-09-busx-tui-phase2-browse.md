<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 2: Browse flow (Objects → Interfaces → Interface)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the Service list, `Enter` drills into Objects (tree) → Interfaces (list, excluding standard housekeeping interfaces) → Interface (three stacked lists: methods / properties-with-values / signals), with single-item auto-skip, a breadcrumb, and `Esc` to go back. `r` refreshes the properties snapshot.

**Architecture:** State gains a navigation **stack** (`Vec<Screen>`) — `Enter` pushes the next screen (with `loading: true` + a spawned async fetch), `Esc` pops; the loop renders the top screen and a breadcrumb derived from the stack. `update` is still pure: data-result `Msg`s populate the top screen and, on arrival, run the **auto-skip** rule (exactly one navigable child ⇒ immediately push the next level). The async `dbus::` core (`object_tree`, `introspect`, `get_all`) backs each screen.

**Tech Stack:** ratatui 0.30 · tui-tree-widget 0.24 · the existing async `dbus::` core · insta snapshots.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (§5 concurrency, §7 navigation + auto-skip, §8 pages + key-hint, §13 testing). Built on Phase 1's Elm core (`src/tui/{state,msg,update,render,app}.rs`).

---

## Conventions

- REUSE SPDX header on new files (copyright `2026 Chen Linxuan <me@black-desk.cn>`, GPL-3.0-or-later). Commit ends with blank line + `Assisted-by: claude:glm-5.2`.
- **Testing:** TestBackend + insta snapshots, driving keys through `update`/`run_loop`. First generation via `INSTA_UPDATE=always cargo test --test tui`; then pinned. `.snap` files are covered by `.reuse/dep5`.
- ratatui 0.30 / tui-tree-widget 0.24 API: confirm against the compiler. Key facts:
  - `tui_tree_widget::{Tree, TreeItem, TreeState}`. `TreeItem::new(id, text, children) -> Result<_>` / `TreeItem::new_leaf(id, text)` (generic over `Identifier: Clone+Eq+Hash`; use `String`). `TreeState::<String>::default()`; `state.key_left/right/up_down(&mut self)`, `state.select(Vec<Identifier>)`. Render: `frame.render_stateful_widget(Tree::new(&items).block(block), area, &mut state)`. Selected id: `state.selected() -> Option<&Vec<Identifier>>`.
  - `dbus::tree::object_tree(&conn, service) -> async Result<ObjectNode>` (ObjectNode `{ path: String, children: Vec<ObjectNode> }`).
  - `dbus::introspect::introspect(&conn, service, object) -> async Result<zbus_xml::Node>` (Node.interfaces() → `&[Interface]`; Interface.name()/methods()/signals()/properties(); Method.name()/args(); Property.name()/ty()/access()).
  - `dbus::property::get_all(&conn, service, object, iface) -> async Result<Vec<(String, OwnedValue)>>`.
  - `crate::value::pretty::pretty(&Value) -> String`.
- **Standard interfaces excluded** from the Interfaces list: `org.freedesktop.DBus.Introspectable`, `org.freedesktop.DBus.Properties`, `org.freedesktop.DBus.Peer` (match by prefix `org.freedesktop.DBus.` is too broad — keep Introspectable/Properties/Peer + also `org.freedesktop.DBus` itself if present; exclude any whose name starts with `org.freedesktop.DBus`).

## File structure (after Phase 2)

- **Modify** `src/tui/state.rs` — `State` gets a `screens: Vec<Screen>` stack (Phase 1's single `screen` folds into the stack's top); `Screen` gains `Objects`/`Interfaces`/`Interface` variants.
- **Modify** `src/tui/msg.rs` — add `ObjectsLoaded`, `InterfacesLoaded`, `PropertiesLoaded` result variants + a `Back` (pop) and `Refresh` message where ergonomic.
- **Modify** `src/tui/update.rs` — `Enter`/`Esc`/`r` per screen; data-result handlers; auto-skip.
- **Modify** `src/tui/render.rs` — breadcrumb bar + render Objects/Interfaces/Interface; per-screen key-hint.
- **Modify** `src/tui/app.rs` — spawn the right fetch when a screen is pushed (helper `spawn_for(top, conn, tx)`).
- **Modify** `src/lib.rs` — no change (modules already exposed).
- **Modify** `tests/tui.rs` — snapshots for each screen + a drill-down loop test with auto-skip.
- **Modify** `Cargo.toml` — add `tui-tree-widget = "0.24"`.

---

## Task 1: Dependencies + navigation stack scaffold

**Files:** Modify `Cargo.toml`, `src/tui/state.rs`, `src/tui/msg.rs`, `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

This task introduces the screen STACK (replacing Phase 1's single `screen` field) and the three new `Screen` variants (as empty/loading stubs), keeps Phase 1 behaviour (Service list) working, and updates `render` to draw a breadcrumb. No tree/interfaces/interface rendering yet — those are Tasks 2–4.

- [ ] **Step 1: Add the tree-widget dependency**

In `Cargo.toml` `[dependencies]` add:

```toml
tui-tree-widget = "0.24"
```

- [ ] **Step 2: Convert `State.screen` to a stack**

In `src/tui/state.rs`, replace the `screen` field with a stack and add the three new variants. The full new file:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state (spec §6, §7). A navigation stack of `Screen`s; `render`
//! draws the top screen + a breadcrumb. `update`/`render` read only this.

use crate::dbus::types::{ObjectNode, ServiceInfo};
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
/// `items` is the `tui-tree-widget` representation built from it.
pub struct ObjectsScreen {
    pub service: String,
    pub tree: ObjectNode,
    pub items: Vec<tui_tree_widget::TreeItem<'static, String>>,
    pub state: TreeState<String>,
    pub loading: bool,
    pub error: Option<String>,
}

/// The non-standard interfaces of one object.
pub struct InterfacesScreen {
    pub service: String,
    pub object: String,
    pub names: Vec<String>,
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

#[derive(Clone, Copy, PartialEq, Eq, Default)]
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
}
```

> `TreeItem<'static, String>`: the `'static` works because `TreeItem` borrows its `text` — we pass owned `String`s, so use `TreeItem::new_leaf(id, text)` where `text: Into<Text<'static>>` (an owned `String` satisfies this). If the lifetime fights back, build `TreeItem<'static, String>` from owned strings (the `new`/`new_leaf` signatures take `T: Into<Text<'text>>`; `String: Into<Text<'static>>`).

- [ ] **Step 3: Update `Msg` for the new screens**

In `src/tui/msg.rs`, add result variants for each new screen's data and a `Back` (Esc) / `Refresh` (r) ergonomic if useful. The full new file:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update` (spec §6).

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crossterm::event::KeyEvent;
use zbus_xml::Node;
use zvariant::OwnedValue;

pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),

    ServicesLoaded(Result<Vec<ServiceInfo>, String>),
    ObjectsLoaded(Result<ObjectNode, String>),
    /// (service, object, the introspection node)
    InterfacesLoaded(String, String, Result<Node<'static>, String>),
    /// (interface name) PropertiesChanged-style refresh result
    PropertiesLoaded(Result<Vec<(String, OwnedValue)>, String>),
}
```

- [ ] **Step 4: Update `update` — keep Service working; route data results to the top screen**

`update` now dispatches key handling by the TOP screen's variant, and data results by variant. For Task 1, only the Service key handling (from Phase 1) + `Esc`-pops-the-stack + the data-result routing stubs (which populate the top screen but don't yet auto-skip) need to compile. Full new `src/tui/update.rs`:

```rust
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
        Msg::ServicesLoaded(res) => match state.top_mut() {
            Screen::Service(s) => load_services(s, res),
            _ => {}
        },
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
        KeyCode::Down | KeyCode::Char('j') => {
            if !s.services.is_empty() {
                s.selected = (s.selected + 1).min(s.services.len() - 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if !s.services.is_empty() {
                s.selected = s.selected.saturating_sub(1);
            }
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
```

> Note: the Phase-1 `State` had a top-level `quit` and a `screen`; the tests from Phase 1 referenced `state.screen` and `state.quit`. `state.quit` still exists. But `state.screen` is gone (it's `state.screens` now) — **update the Phase-1 tests** (`tests/tui.rs`) accordingly: replace `&state.screen` reads with `state.top()`, and `State { screen, quit }` literals with the new stack shape. Step 6 does this.

- [ ] **Step 5: Update `render` — breadcrumb + route to the top screen**

`src/tui/render.rs` gains a breadcrumb bar at the top and dispatches to per-screen renderers (Service reused from Phase 1; the new three render a loading placeholder for Task 1). Full new file:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering (spec §6, §8). Reads `&State`; draws breadcrumb + top screen
//! + key-hint. Nothing else.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::tui::state::{Screen, ServiceScreen, State};

pub fn render(frame: &mut Frame, state: &State) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let (crumb, main, footer) = (chunks[0], chunks[1], chunks[2]);

    render_breadcrumb(frame, crumb, state);
    match state.top() {
        Screen::Service(s) => render_service(frame, main, s),
        Screen::Objects(_) => render_placeholder(frame, main, "Objects"),
        Screen::Interfaces(_) => render_placeholder(frame, main, "Interfaces"),
        Screen::Interface(_) => render_placeholder(frame, main, "Interface"),
    }
    render_keyhint(frame, footer, state.top());
}

fn render_breadcrumb(frame: &mut Frame, area: Rect, state: &State) {
    let parts: Vec<String> = state.screens.iter().map(screen_crumb).collect();
    let text = parts.join(" > ");
    frame.render_widget(Paragraph::new(text), area);
}

fn screen_crumb(s: &Screen) -> String {
    match s {
        Screen::Service(_) => "services".to_string(),
        Screen::Objects(o) => o.service.clone(),
        Screen::Interfaces(i) => format!("{} {}", i.service, i.object),
        Screen::Interface(i) => format!("{}:{}:{}", i.service, i.object, i.interface),
    }
}

fn render_placeholder(frame: &mut Frame, area: Rect, name: &str) {
    frame.render_widget(Paragraph::new(format!("{name} (loading…)")), area);
}

fn render_service(frame: &mut Frame, area: Rect, s: &ServiceScreen) {
    let title = if s.loading { "Services (loading…)" } else { "Services" };
    let block = Block::default().borders(Borders::ALL).title(title);

    if let Some(err) = &s.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }

    let items: Vec<ListItem> = s
        .services
        .iter()
        .map(|sv| {
            let pid = sv.pid.map(|p| p.to_string()).unwrap_or_default();
            let proc = sv.process.clone().unwrap_or_default();
            ListItem::new(Line::from(format!("{:<32} {:>7} {}", sv.name, pid, proc)))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !s.services.is_empty() {
        list_state.select(Some(s.selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_keyhint(frame: &mut Frame, area: Rect, screen: &Screen) {
    let hint = match screen {
        Screen::Service(_) => "↑↓ select · Enter open · q quit · ? help",
        Screen::Objects(_) => "↑↓/→← navigate · Enter open · Esc back · q quit",
        Screen::Interfaces(_) => "↑↓ select · Enter open · Esc back · q quit",
        Screen::Interface(_) => "Tab switch · ↑↓ select · r refresh · Esc back · q quit",
    };
    frame.render_widget(Paragraph::new(hint), area);
}
```

- [ ] **Step 6: Fix the Phase-1 tests for the new stack shape + regenerate snapshots**

In `tests/tui.rs`: the helper `selected_of` and the loading/error/loop tests read `state.screen` / build `State { screen, quit }`. Update them:
- `selected_of(state)`: `match state.top() { Screen::Service(s) => s.selected, _ => 0 }` (import `busx::tui::Screen`).
- `service_screen_loading_state`: use `State::loading_service()` (already does).
- `service_screen_error_state`: build via `State { screens: vec![Screen::Service(ServiceScreen { services: vec![], selected: 0, loading: false, error: Some(...) })], quit: false }`.
- The `loop_loads_services_then_navigates` test: `app.state.top()` instead of `state.screen`; `selected_of(&app.state)` still works.
- The Service snapshots now have an extra top breadcrumb line ("services") — regenerate them.

Run:

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test --test tui
```

Expected: PASS; the Service snapshots regenerated (now include the breadcrumb line); all CLI e2e still pass.

- [ ] **Step 7: Build + full suite + commit**

Run: `cargo build && cargo test -q && cargo clippy --all-targets -- -D warnings`
Expected: all green.

```bash
git add Cargo.toml Cargo.lock src/tui/ tests/tui.rs tests/snapshots/
git commit -m "feat(busx): tui navigation stack + breadcrumb (browse scaffold)

Assisted-by: claude:glm-5.2"
```

---

## Task 2: Objects screen — object-path tree (tui-tree-widget)

**Files:** Modify `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

`Enter` on a Service pushes `ObjectsScreen { service, loading:true }` and the loop spawns `dbus::tree::object_tree`. `ObjectsLoaded` builds the `TreeItem`s from the `ObjectNode`, populates the screen, and runs auto-skip (exactly one object ⇒ push Interfaces for it).

- [ ] **Step 1: Write the failing Objects snapshot + drill test**

Append to `tests/tui.rs`:

```rust
use busx::dbus::types::ObjectNode;

fn obj(path: &str, children: &[ObjectNode]) -> ObjectNode {
    ObjectNode { path: path.to_string(), children: children.to_vec() }
}

#[test]
fn objects_screen_renders_tree() {
    let tree = obj("/", &[obj("/org", &[obj("/org/busx", &[])]), obj("/foo", &[])]);
    let items = busx::tui::tree_items(&tree);
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(busx::tui::state::ObjectsScreen {
            service: "org.busx.Test".into(),
            tree,
            items,
            state: tui_tree_widget::TreeState::default(),
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 48, 9));
}
```

> This needs `busx::tui::tree_items(&ObjectNode) -> Vec<TreeItem<String>>` (a pure helper that turns the path-tree into tui-tree-widget items, with each item's identifier = its full path and text = the last path segment). It's defined in Step 3 and re-exported.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test tui objects_screen_renders_tree`
Expected: FAIL (compile error — `tree_items` / `ObjectsScreen` fields not reachable).

- [ ] **Step 3: Add `tree_items` helper + make `ObjectsScreen` fields `pub`**

In `src/tui/state.rs`: ensure `ObjectsScreen` fields are `pub` (they are, from Task 1). Add a module-level helper in `src/tui/mod.rs` (or `state.rs`) — put it in `state.rs` and re-export:

Add to `src/tui/state.rs`:

```rust
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
        // new() errors on duplicate child ids; our paths are unique, so unwrap is fine.
        tui_tree_widget::TreeItem::new(node.path.clone(), text, children).unwrap()
    }
}
```

In `src/tui/mod.rs` add `pub use state::tree_items;`.

- [ ] **Step 4: Render the Objects tree**

In `src/tui/render.rs`, replace `render_placeholder(frame, main, "Objects")` with a call to a new `render_objects(frame, main, o)`:

```rust
fn render_objects(frame: &mut Frame, area: Rect, o: &crate::tui::state::ObjectsScreen) {
    let title = if o.loading { "Objects (loading…)" } else { "Objects" };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &o.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let mut state = o.state.clone();
    let tree = tui_tree_widget::Tree::new(&o.items).block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(tree, area, &mut state);
}
```

(And update the `match state.top()` arm: `Screen::Objects(o) => render_objects(frame, main, o),`.)

- [ ] **Step 5: Handle `ObjectsLoaded` + Service `Enter` + auto-skip in `update`**

In `src/tui/update.rs`:

- Service `Enter`: push `ObjectsScreen { service: <selected name>, tree: empty, items: empty, state: default, loading: true, error: None }`. (The actual fetch is spawned by the loop in Task 2 Step 6; `update` only pushes the screen — it can't spawn. So `update` pushes the screen and the LOOP, seeing a new loading screen on top, spawns the fetch. To keep `update` pure, push a screen with `loading: true`; the loop's `spawn_for` (Step 6) notices a loading top screen and spawns.)
- `Msg::ObjectsLoaded(res)`: if the top screen is `Objects`, populate `tree`/`items`/`loading=false` (or `error`). Then auto-skip: if `tree` has exactly one child path, push `InterfacesScreen { service, object: that path, loading: true }`.

```rust
        Msg::ObjectsLoaded(res) => match state.top_mut() {
            Screen::Objects(o) => {
                o.loading = false;
                match res {
                    Ok(root) => {
                        o.items = crate::tui::state::tree_items(&root);
                        o.tree = root;
                        // Auto-skip: exactly one object ⇒ drill into Interfaces.
                        if o.tree.children.len() == 1 {
                            let svc = o.service.clone();
                            let path = o.tree.children[0].path.clone();
                            push_interfaces(state, svc, path);
                        }
                    }
                    Err(e) => o.error = Some(e),
                }
            }
            _ => {}
        },
```

Add `push_interfaces` (used by auto-skip here and by Objects `Enter` in Task 3):

```rust
/// Push an Interfaces screen for (service, object) in loading state.
fn push_interfaces(state: &mut State, service: String, object: String) {
    state.screens.push(Screen::Interfaces(crate::tui::state::InterfacesScreen {
        service, object, names: vec![], selected: 0, loading: true, error: None,
    }));
}
```

And in `update_service_key`, handle `Enter`:

```rust
        KeyCode::Enter => {
            // Push an Objects screen for the selected service (loading). The loop
            // spawns object_tree when it sees a loading Objects screen on top.
            if let Some(sv) = s.services.get(s.selected).cloned() {
                state.screens.push(Screen::Objects(crate::tui::state::ObjectsScreen {
                    service: sv.name,
                    tree: crate::dbus::types::ObjectNode { path: "/".into(), children: vec![] },
                    items: vec![],
                    state: tui_tree_widget::TreeState::default(),
                    loading: true,
                    error: None,
                }));
            }
        }
```

(`update_service_key` now needs `state: &mut State` to push — change its signature to `fn update_service_key(state: &mut State, s: &mut ServiceScreen, code: KeyCode)` and pass both from `update_key`, OR move the Enter handling into `update_key` before the `match state.top_mut()`. Simplest: handle Enter in `update_key` directly when the top is Service.)

> Refactor: in `update_key`, after the Esc/q handling, do:
> ```rust
> if k.code == KeyCode::Enter {
>     if let Screen::Service(s) = state.top_mut() {
>         if let Some(sv) = s.services.get(s.selected).cloned() {
>             state.screens.push(Screen::Objects(/* loading */));
>         }
>     }
>     return;
> }
> ```
> then the `match state.top_mut()` handles nav keys only.

- [ ] **Step 6: Loop spawns the Objects fetch on push**

In `src/tui/app.rs`, the loop must spawn the right fetch when a loading screen is on top. Add a `spawn_for(top, conn, tx)` helper called after each `update` in `run_loop` (only when the top screen is in a `loading` state it hasn't spawned for yet). To avoid re-spawning every frame, track a `spawned: bool` per screen OR spawn only when `loading && !spawned`. Simplest: give each loading screen a `loading: bool` that the loop flips to a sentinel — but `update` sets `loading=false` on result. 

Cleanest: the loop spawns when it observes a transition to a loading screen. Add to `App` a `last_top_kind` (an enum of screen kinds) and spawn when the top kind changes to a loading screen. Concretely, after `update`, call `maybe_spawn(&mut self)`:

```rust
impl App {
    pub fn run_loop<B: Backend>(&mut self, terminal: &mut Terminal<B>, mut events: impl Iterator<Item = Msg>) -> Result<()>
    where crate::error::Error: From<<B as Backend>::Error>,
    {
        let mut spawned_for = None::<ScreenKind>;
        while !self.state.quit {
            terminal.draw(|f| render(f, &self.state))?;
            // Spawn the fetch for a freshly-shown loading screen.
            let kind = screen_kind(self.state.top());
            if kind != spawned_for {
                if let Some(spawn) = spawn_msg(self.state.top()) {
                    (spawn)(self.tx.clone(), self.conn.clone());
                }
                spawned_for = kind;
            }
            match events.next() {
                Some(msg) => { update(&mut self.state, msg); spawned_for = if just_pushed { None } else { spawned_for }; }
                None => break,
            }
        }
        Ok(())
    }
}
```

> This is getting intricate; a cleaner factoring: have `update` return an `Option<Effect>` describing what to fetch (e.g. `Effect::FetchObjects(service)`, `Effect::FetchInterfaces(svc,obj)`, `Effect::FetchProperties(svc,obj,iface)`), and the loop spawns from the returned effect. This keeps `update` pure (returns data, no IO) and the loop does the spawning — no "detect loading screen" heuristic. **Adopt this:** change `update` to `pub fn update(state, msg) -> Option<Effect>`, where pushing a screen returns the matching `Effect`, and the loop spawns from it.

Define in `src/tui/msg.rs`:

```rust
/// A side effect `update` requests (the loop performs the IO).
pub enum Effect {
    FetchServices,
    FetchObjects(String),
    FetchInterfaces(String, String),
    FetchProperties(String, String, String),
}
```

Change `update` to return `Option<Effect>`: pushing Objects returns `Effect::FetchObjects(svc)`; auto-skip pushing Interfaces returns `Effect::FetchInterfaces(svc,obj)`; Task 3/4 add their effects; `ServicesLoaded`/etc. handlers return `None`. The loop:

```rust
match events.next() {
    Some(msg) => {
        if let Some(effect) = update(&mut self.state, msg) {
            run_effect(effect, self.conn.clone(), self.tx.clone());
        }
    }
    None => break,
}
```

`run_effect` spawns the matching `dbus::*` call and sends the result `Msg`. (The initial Services fetch on startup is `run_effect(Effect::FetchServices, ...)` in `run`.)

> This `Effect` refactor is the clean answer to "loop spawns the right fetch." Apply it here (Task 2) — it also retro-cleans the initial Services fetch (replace `spawn_list_names` with `run_effect(Effect::FetchServices, ...)`).

- [ ] **Step 7: Generate + pin the Objects snapshot, run the suite**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q
```

Expected: the Objects tree snapshot is generated (shows `/org`, `/foo` as top-level, `/org` expandable) and pinned; all green.

- [ ] **Step 8: Commit**

```bash
git add src/tui/ tests/tui.rs tests/snapshots/ Cargo.toml Cargo.lock
git commit -m "feat(busx): tui Objects screen — object-path tree + Enter + auto-skip

Assisted-by: claude:glm-5.2"
```

---

## Task 3: Interfaces screen — non-standard interface list

**Files:** Modify `src/tui/update.rs`, `src/tui/app.rs`, `tests/tui.rs`. (`render.rs` Interfaces arm already shows a list once implemented.)

`Enter` on a tree node (Objects) pushes `InterfacesScreen { service, object: selected path, loading }`; `InterfacesLoaded` filters out standard interfaces, populates `names`, and auto-skips if exactly one remains. `Enter` on an interface pushes `InterfaceScreen`.

- [ ] **Step 1: Write the failing Interfaces snapshot + filter test**

Append to `tests/tui.rs`:

```rust
#[test]
fn interfaces_screen_lists_non_standard() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(busx::tui::state::InterfacesScreen {
            service: "org.busx.Test".into(),
            object: "/org/busx/Test".into(),
            names: vec!["org.busx.Test".into()],
            selected: 0, loading: false, error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 44, 7));
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --test tui interfaces_screen_lists_non_standard` (FAIL: render still placeholder).

- [ ] **Step 3: Render the Interfaces list**

In `render.rs`, replace `render_placeholder(frame, main, "Interfaces")` with `render_interfaces(frame, main, i)`:

```rust
fn render_interfaces(frame: &mut Frame, area: Rect, i: &crate::tui::state::InterfacesScreen) {
    let title = if i.loading { "Interfaces (loading…)" } else { "Interfaces" };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &i.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let items: Vec<ListItem> = i.names.iter().map(|n| ListItem::new(Line::from(n.clone()))).collect();
    let list = List::new(items).block(block).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ls = ListState::default();
    if !i.names.is_empty() { ls.select(Some(i.selected)); }
    frame.render_stateful_widget(list, area, &mut ls);
}
```

- [ ] **Step 4: Handle `InterfacesLoaded` + Objects `Enter` + auto-skip + Interfaces `Enter`**

In `update.rs`:
- Objects `Enter` (tree node selected): push `InterfacesScreen { service, object: selected_path, loading }`, return `Effect::FetchInterfaces(svc, obj)`.
- `Msg::InterfacesLoaded(svc, obj, res)`: if top is Interfaces matching (svc,obj), parse `Node`, filter standard interfaces (`!name.starts_with("org.freedesktop.DBus")`), set `names`, `loading=false`. Auto-skip: if exactly one name, push `InterfaceScreen { service, object, interface: that name, loading }`, return `Effect::FetchProperties(...)`.
- Interfaces `Enter`: push `InterfaceScreen`, return `Effect::FetchProperties`.

The `Effect`/`run_effect` from Task 2 handles `FetchInterfaces` (calls `dbus::introspect::introspect`, sends `InterfacesLoaded`) and `FetchProperties` (calls `dbus::property::get_all`, sends `PropertiesLoaded`).

Standard-interface exclusion helper:

```rust
fn is_standard_interface(name: &str) -> bool {
    name.starts_with("org.freedesktop.DBus")
}
```

- [ ] **Step 5: Generate + pin snapshot, run suite**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q
```

- [ ] **Step 6: Commit**

```bash
git add src/tui/ tests/tui.rs tests/snapshots/
git commit -m "feat(busx): tui Interfaces screen — non-standard list + auto-skip

Assisted-by: claude:glm-5.2"
```

---

## Task 4: Interface screen — three columns (methods / properties+values / signals) + `r` refresh

**Files:** Modify `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

The Interface screen shows three stacked lists: methods (name + in-signature), properties (name + signature + pretty value from the GetAll snapshot), signals (name + signature). `Tab` cycles focus among the three; `↑↓` move within the focused list; `r` re-fetches the properties snapshot.

- [ ] **Step 1: Write the failing Interface snapshot**

Append to `tests/tui.rs`:

```rust
#[test]
fn interface_screen_renders_three_columns() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
            service: "org.busx.Test".into(),
            object: "/org/busx/Test".into(),
            interface: "org.busx.Test".into(),
            methods: vec![("BumpVolume".into(), "".into()), ("Join".into(), "as".into())],
            properties: vec![("volume".into(), "d".into(), "readwrite".into()), ("name".into(), "s".into(), "read".into())],
            signals: vec![],
            prop_values: vec![("volume".into(), "0.5".into()), ("name".into(), "\"busx-test\"".into())],
            focus: busx::tui::state::InterfaceFocus::Properties,
            selected: [0, 1, 0],
            loading: false, error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 60, 16));
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --test tui interface_screen_renders_three_columns` (FAIL).

- [ ] **Step 3: Render the Interface screen (three columns)**

In `render.rs`, replace `render_placeholder(frame, main, "Interface")` with `render_interface(frame, main, i)`. Layout: split `main` into three vertical chunks (methods / properties / signals); each is a `List` with a `Block` title; the focused one gets a distinct border style. Properties show `name  sig  value`:

```rust
fn render_interface(frame: &mut Frame, area: Rect, i: &crate::tui::state::InterfaceScreen) {
    if let Some(err) = &i.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")), area);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(33), Constraint::Percentage(33)])
        .split(area);

    let title = format!("{}:{}", i.object, i.interface);
    // Methods
    let m_items: Vec<ListItem> = i.methods.iter()
        .map(|(n, sig)| ListItem::new(Line::from(format!("{n}  {sig}")))).collect();
    render_sub_list(frame, chunks[0], "methods", m_items, i.selected[0], i.focus == InterfaceFocus::Methods, &title);
    // Properties (name, sig, value)
    let p_items: Vec<ListItem> = i.properties.iter().enumerate()
        .map(|(idx, (n, sig, _acc))| {
            let val = i.prop_values.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap_or_default();
            ListItem::new(Line::from(format!("{n}  {sig}  {val}")))
        }).collect();
    render_sub_list(frame, chunks[1], "properties", p_items, i.selected[1], i.focus == InterfaceFocus::Properties, "");
    // Signals
    let s_items: Vec<ListItem> = i.signals.iter()
        .map(|(n, sig)| ListItem::new(Line::from(format!("{n}  {sig}")))).collect();
    render_sub_list(frame, chunks[2], "signals", s_items, i.selected[2], i.focus == InterfaceFocus::Signals, "");
}

fn render_sub_list(frame: &mut Frame, area: Rect, title: &str, items: Vec<ListItem>, selected: usize, focused: bool, block_title: &str) {
    let style = if focused { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
    let block = Block::default().borders(Borders::ALL).title(if block_title.is_empty() { title.to_string() } else { format!("{title}: {block_title}") });
    let list = List::new(items).block(block).highlight_style(style);
    let mut ls = ListState::default();
    ls.select(Some(selected));
    frame.render_stateful_widget(list, area, &mut ls);
}
```

(Import `InterfaceFocus` in render.rs.)

- [ ] **Step 4: Handle `PropertiesLoaded` + Interface keys (Tab, ↑↓, r)**

In `update.rs`:
- `Msg::PropertiesLoaded(res)`: if top is Interface, set `prop_values` (name → pretty via `value::pretty::pretty`), `loading=false` (or error).
- Interface keys: `Tab` cycles `focus` (Methods→Properties→Signals→Methods); `↑↓` move `selected[focus_index]` within that column's list (clamped); `r` returns `Effect::FetchProperties(svc, obj, iface)` (re-fetch). `Enter` is a no-op in Phase 2 (Phase 3 wires method/property actions).
- Building the InterfaceScreen on push: from the Interfaces list, `Enter` pushes `InterfaceScreen { service, object, interface, methods/properties/signals: parsed from the introspect Node (cached on the InterfacesScreen or re-fetched), loading, ... }` + `Effect::FetchProperties`. 

  Where do methods/properties/signals come from? The introspect `Node` was fetched for the Interfaces screen. Cache the parsed members on `InterfacesScreen` (add `members: Vec<(String, String /*method*/, ...)>` — or simpler, re-introspect on Interface push). Simplest: on Interfaces `Enter`, push InterfaceScreen with members parsed from the `Node` we already have (store the `Node` on `InterfacesScreen`). Add `node: Option<zbus_xml::Node<'static>>` to `InterfacesScreen` and populate it in `InterfacesLoaded`; on `Enter`, parse methods/properties/signals from it into the InterfaceScreen. This avoids a second introspect round-trip.

- [ ] **Step 5: Generate + pin snapshot, run suite**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 6: Add a drill-down loop test with auto-skip**

Append to `tests/tui.rs` a loop test that drives `Enter` from Service through Objects (auto-skip) to Interfaces to Interface, with scripted data results, and snapshots the final Interface frame. (This is the Phase-2 capstone: it exercises the whole nav stack + auto-skip + Effect spawning logic via the loop.) Use `App` + scripted `Msg`s.

- [ ] **Step 7: Commit**

```bash
git add src/tui/ tests/tui.rs tests/snapshots/
git commit -m "feat(busx): tui Interface screen — three columns + property values + r refresh

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** Objects tree (§8) ✓ T2; Interfaces list excluding standard (§7/§8) ✓ T3; Interface three columns + property GetAll snapshot + r refresh (§8) ✓ T4; navigation stack push/Esc-pop (§7) ✓ T1; single-item auto-skip (§7) ✓ T2/T3; breadcrumb + per-screen key-hint (§8) ✓ T1. Gaps deferred: method/property/signal ACTION buttons + detail/result screens (phase 3), monitors (phase 4), copy-as (phase 5).
- **Placeholders:** the `Effect` refactor (Task 2 Step 6) is described with concrete code; the Interface member-parsing (Task 4 Step 4) names the source (`zbus_xml::Node` cached on InterfacesScreen) — verify the executor implements the parse (Method/Property/Signal name+signature) concretely rather than leaving a stub.
- **Type consistency:** `Screen` variants + their screen structs defined T1, used T2–T4. `Msg` result variants defined T1. `Effect` defined T2 (msg.rs), used T2–T4 + loop. `tree_items` defined T2, used T2. `InterfaceFocus` defined T1, used T4.
- **Risk:** the `Effect` refactor (update returns `Option<Effect>`) is the cleanest way to keep `update` pure while letting the loop spawn the right fetch — but it's a structural change to the Phase-1 `update` signature; confirm all Phase-1 callers (tests + loop) are updated. The Interface member-parse (Task 4) must be concrete, not a stub.

---

## Roadmap — remaining plans

3. **Call / read / write** — method-call detail (per-arg `tui-input`) + result; property get/set detail + result; the Interface screen's right-column action buttons.
4. **Listen + cancel** — signal/property/method listen; result streaming; `Esc`-leaves-stops. *(Also migrates `ops/monitor.rs` to the async core and removes `src/conn.rs`.)*
5. **copy-as + clipboard** — dbus-send/busctl/qdbus/gdbus; `arboard`; copy-as popup.
6. **Polish** — `?` help, error toasts, JSON view toggle, column sizing for long names, empty/edge states, snapshot coverage.
