<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 0: Extract async `dbus/` core + migrate CLI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a shared async `src/dbus/` core (typed structs, no printing), migrate the request/response CLI ops onto it, and adopt `zbus_xml` for introspection — all existing CLI e2e tests staying green. No TUI yet.

**Architecture:** New `src/dbus/` holds every D-Bus operation as an `async fn` returning typed data (`ServiceInfo`, `ObjectNode`, `Vec<OwnedValue>`, `zbus_xml::Node`). The CLI `ops/*` become thin: `async_global_executor::block_on` the core, then render (unchanged human/JSON output). `monitor` stays on its current blocking stream for now (its full async migration belongs with the TUI monitor in a later phase); only its pure `build_match_rule` is extracted. `roxmltree` + `src/introspect.rs` are dropped in favour of `zbus_xml`.

**Tech Stack:** Rust 2024 · zbus 5 (async) · zbus_xml 5.2 · async-global-executor · zvariant 5 · serde_json · clap 4.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (this plan = §14 phase 0).

---

## Conventions

- **Testing is e2e-only** (no unit tests). The existing `tests/*.rs` files drive the `busx` binary against a shared `dbus-daemon` fixture (`tests/common/mod.rs` registers `org.busx.Test` at `/org/busx/Test` and `/org/busx/Test/sub`, with `volume`/`name`/`counts`(a{uu})/`hints`(a{sv})/`take_hints`/`join`/`bump_volume`). For each migrated op, the guard is: the matching `tests/<op>.rs` keeps passing. Run the whole suite with `cargo test`.
- **Every commit message ends with a blank line then exactly:** `Assisted-by: claude:glm-5.2`
- **Never change the CLI's observable output.** The whole point of this phase is a behaviour-preserving refactor; the e2e tests pin the output. If a test fails after a migration, fix the migration until it passes — do not touch the test.
- **`zbus` async API:** the blocking code's `zbus::blocking::*` maps to async `zbus::*` with `.await` added (e.g. `Connection::session().await`, `DBusProxy::new(conn).await`, `proxy.call_method(m, &body).await`, `proxy.get_all(iface).await`). Address connect uses `zbus::connection::Builder::address(addr)?.build().await?`.
- **`zbus_xml` API (confirmed for 5.x):** `Node::from_reader(reader: impl Read) -> Result<Node<'static>>` (owned); `Node::interfaces() -> &[Interface]`, `Node::nodes() -> &[Node]`, `Node::name() -> Option<&str>`; `Interface::name() -> InterfaceName<'_>`, `.methods()/signals()/properties()`; `Method/Signal::name() -> MemberName<'_>`, `.args() -> &[Arg]`; `Property::name() -> PropertyName<'_>`, `.ty() -> &CompleteType`, `.access() -> PropertyAccess`; `Arg::name() -> Option<&str>`, `.ty() -> &Signature`, `.direction() -> Option<ArgDirection>`. All `Display` via `.to_string()`; name types are `zbus_names` newtypes that `Display` as the plain string.
- **zvariant lifetimes:** a reply body deserializes to `Value<'_>` borrowing the message. To return values across the function boundary (and to store in TUI state later), convert to owned via `Value::try_to_owned()` → `zvariant::OwnedValue` (which `Deref`s to `Value`, so `crate::value::pretty::pretty(&owned)` / `crate::value::decode::to_tagged(&owned)` work unchanged). If the method is named differently in the resolved zvariant version, use the equivalent owned conversion — the e2e `call`/`get`/`set` tests guard the rendered output.

## File structure (after Phase 0)

- **Create** `src/dbus/mod.rs` — `pub mod call/conn/introspect/list/monitor/property/tree/types;`
- **Create** `src/dbus/types.rs` — `ServiceInfo`, `ObjectNode`
- **Create** `src/dbus/conn.rs` — async `connect` (session→system fallback)
- **Create** `src/dbus/list.rs` — `list_names` (incl. pid/process enrichment)
- **Create** `src/dbus/introspect.rs` — `introspect` → `zbus_xml::Node`
- **Create** `src/dbus/tree.rs` — `object_tree` (recursive, via `zbus_xml`)
- **Create** `src/dbus/call.rs` — `call_method` → `Vec<OwnedValue>`
- **Create** `src/dbus/property.rs` — `get_all` / `get_one` / `set`
- **Create** `src/dbus/monitor.rs` — pure `build_match_rule` (moved out of `ops/monitor.rs`)
- **Modify** `src/main.rs` — add `mod dbus;`
- **Modify** `src/ops/{list,introspect,tree,call,property}.rs` — call core via `block_on`, render from typed data
- **Modify** `src/ops/monitor.rs` — call `dbus::monitor::build_match_rule`
- **Modify** `Cargo.toml` — add `zbus_xml`, `async-global-executor`; remove `roxmltree`
- **Delete** `src/introspect.rs` (roxmltree parser; superseded by `zbus_xml`)
- **Keep** `src/conn.rs` (still used by the blocking `monitor` op and `complete.rs`; removed in the monitor's async phase)
- **Keep** `src/ops/monitor.rs` blocking stream as-is (aside from calling the shared `build_match_rule`)

> Deviation from the spec end-state, justified: the spec says delete `src/conn.rs` and async-ify monitor. Monitor streams over `zbus::blocking::MessageIterator`; its async rework (BecomeMonitor → async message stream + cancel) is inseparable from the TUI monitor UI, so it lands in the monitor phase, not here. Phase 0 only extracts the pure match-rule helper and leaves the blocking monitor intact.

---

## Task 1: Dependencies + module skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/dbus/mod.rs`, `src/dbus/{types,conn,list,introspect,tree,call,property,monitor}.rs` (empty stubs)
- Modify: `src/main.rs`

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` `[dependencies]`, add (keep `roxmltree` for now — removed in Task 10):

```toml
zbus_xml = "5"
async-global-executor = "2"
```

- [ ] **Step 2: Create the module skeleton**

`src/dbus/mod.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared async D-Bus core (spec §3). All concrete D-Bus operations live here as
//! `async fn`s returning typed data (no printing). Both the CLI (`ops/`) and the
//! future TUI consume this module.

pub mod call;
pub mod conn;
pub mod introspect;
pub mod list;
pub mod monitor;
pub mod property;
pub mod tree;
pub mod types;
```

Create each listed submodule file with just the SPDX header (3 lines: `// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>` / blank / `// SPDX-License-Identifier: GPL-3.0-or-later`) so the module compiles empty. Example `src/dbus/types.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later
```

- [ ] **Step 3: Wire the module into main**

In `src/main.rs`, add `mod dbus;` to the module declaration block (keep alphabetical order with the other `mod` lines).

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: builds with no errors (empty submodules).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/dbus/ src/main.rs
git commit -F - <<'EOF'
refactor(busx): scaffold shared async dbus core module

Empty src/dbus/ module wired into main; adds zbus_xml and
async-global-executor deps. No behaviour change yet.

Assisted-by: claude:glm-5.2
EOF
```

---

## Task 2: Core types (`dbus/types.rs`)

**Files:**
- Modify: `src/dbus/types.rs`

- [ ] **Step 1: Define the two owned data structs**

`src/dbus/types.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned data types returned by the async core (spec §4). Introspection reuses
//! `zbus_xml` types directly; only these non-introspection results are ours.

/// One bus name with best-effort PID + process enrichment.
pub struct ServiceInfo {
    pub name: String,
    pub pid: Option<u64>,
    pub process: Option<String>,
}

/// A node in an object-path tree (the result of recursively introspecting a
/// service). `path` is the absolute object path; `children` are sub-objects.
pub struct ObjectNode {
    pub path: String,
    pub children: Vec<ObjectNode>,
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/dbus/types.rs
git commit -m "refactor(busx): add ServiceInfo/ObjectNode core types

Assisted-by: claude:glm-5.2"
```

---

## Task 3: Async connection (`dbus/conn.rs`)

**Files:**
- Modify: `src/dbus/conn.rs`

- [ ] **Step 1: Port the session→system fallback to async**

`src/dbus/conn.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Async bus connection with the session→system fallback (spec §9), mirroring
//! the blocking `crate::conn::connect`. `--address` > `--system` > session with
//! silent fallback to system (warn on `--verbose`).

use crate::error::{Error, Result};
use zbus::Connection;

pub async fn connect(user: bool, system: bool, address: Option<&str>, verbose: bool) -> Result<Connection> {
    if let Some(addr) = address {
        return Ok(zbus::connection::Builder::address(addr)?.build().await?);
    }
    if system {
        return Ok(Connection::system().await?);
    }
    match Connection::session().await {
        Ok(c) => Ok(c),
        Err(e) if user => Err(Error::Msg(format!("cannot connect to session bus: {e}"))),
        Err(e) => {
            if verbose {
                eprintln!("busx: warning: session bus unavailable ({e}); falling back to system bus");
            }
            Ok(Connection::system().await?)
        }
    }
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/dbus/conn.rs
git commit -m "refactor(busx): add async connect to dbus core

Assisted-by: claude:glm-5.2"
```

---

## Task 4: `list` core + migrate `ops/list.rs`

**Files:**
- Modify: `src/dbus/list.rs`, `src/ops/list.rs`

- [ ] **Step 1: Implement the core `list_names` (with pid/process enrichment)**

`src/dbus/list.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `list_names` — names on the bus + best-effort PID/process (spec §7).

use crate::dbus::types::ServiceInfo;
use crate::error::Result;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

pub async fn list_names(
    conn: &zbus::Connection,
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<Vec<ServiceInfo>> {
    let dbus = DBusProxy::new(conn).await?;
    let mut names: Vec<String> = if activatable {
        dbus.list_activatable_names().await?
    } else {
        dbus.list_names().await?
    }
    .into_iter()
    .map(|n| n.to_string())
    .collect();

    // `--unique` and `--acquired` are mutually-exclusive filters; both set = no filter.
    if unique && !acquired {
        names.retain(|n| n.starts_with(':'));
    } else if acquired && !unique {
        names.retain(|n| !n.starts_with(':'));
    }
    names.sort();

    let mut out = Vec::with_capacity(names.len());
    for n in &names {
        out.push(proc_info(&dbus, n).await);
    }
    Ok(out)
}

/// PID via `GetConnectionUnixProcessID`, process via `/proc/<pid>/comm`. Any
/// failure (bus driver has no PID; non-Linux) degrades to `None`s.
async fn proc_info(dbus: &DBusProxy<'_>, name: &str) -> ServiceInfo {
    let empty = ServiceInfo { name: name.to_string(), pid: None, process: None };
    let bus_name = match BusName::try_from(name) {
        Ok(b) => b,
        Err(_) => return empty,
    };
    let pid = match dbus.get_connection_unix_process_id(bus_name).await {
        Ok(p) => p as u64,
        Err(_) => return empty,
    };
    let process = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim_end_matches('\n').to_string());
    ServiceInfo { name: name.to_string(), pid: Some(pid), process }
}
```

- [ ] **Step 2: Migrate `ops/list.rs` to use the core**

Replace `src/ops/list.rs` with the thin version below. The human table and JSON shapes are unchanged — only the data now comes from `ServiceInfo`.

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx list` — print the names on the bus (spec §7). Thin wrapper: runs the
//! async core under `block_on`, then renders (human table ≤80 cols / JSON array).

use crate::dbus;
use crate::error::Result;
use serde_json::{Value as Json, json};

/// Truncate `s` to `cap` display columns, appending `…` when longer.
fn cap_cell(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        let head: String = s.chars().take(cap.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<()> {
    let names = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::list::list_names(&conn, unique, acquired, activatable).await
    })?;

    if json {
        let arr: Vec<Json> = names
            .iter()
            .map(|n| json!({ "name": n.name, "pid": n.pid, "process": n.process }))
            .collect();
        crate::out::print_json(&json!(arr));
    } else {
        // NAME  PID  PROCESS, total width ≤ 80. PID ≤ 7 digits, PROCESS (from
        // /proc/<pid>/comm) ≤ 15, so NAME capped at 54.
        const NAME_CAP: usize = 54;
        let mut rows: Vec<[String; 3]> = Vec::with_capacity(names.len());
        for n in &names {
            rows.push([
                cap_cell(&n.name, NAME_CAP),
                n.pid.map(|p| p.to_string()).unwrap_or_default(),
                n.process.clone().unwrap_or_default(),
            ]);
        }
        let cols = ["NAME", "PID", "PROCESS"];
        let mut widths = [
            cols[0].chars().count(),
            cols[1].chars().count(),
            cols[2].chars().count(),
        ];
        for r in &rows {
            for (i, cell) in r.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        println!("{:<w0$}  {:<w1$}  {:<w2$}", cols[0], cols[1], cols[2], w0 = widths[0], w1 = widths[1], w2 = widths[2]);
        for r in &rows {
            println!("{:<w0$}  {:<w1$}  {:<w2$}", r[0], r[1], r[2], w0 = widths[0], w1 = widths[1], w2 = widths[2]);
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Run the list e2e**

Run: `cargo test --test list`
Expected: PASS (same JSON `{name,pid,process}` shape and human table as before).

- [ ] **Step 4: Commit**

```bash
git add src/dbus/list.rs src/ops/list.rs
git commit -m "refactor(busx): migrate list onto async dbus core

Assisted-by: claude:glm-5.2"
```

---

## Task 5: `introspect` core (zbus_xml) + migrate `ops/introspect.rs`

**Files:**
- Modify: `src/dbus/introspect.rs`, `src/ops/introspect.rs`
- Delete: `src/introspect.rs`

- [ ] **Step 1: Implement the core `introspect` returning a `zbus_xml::Node`**

`src/dbus/introspect.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `introspect` — call `Introspect` and parse the XML with `zbus_xml` (spec §6).
//! `Node::from_reader` yields an owned (`'static`) tree.

use crate::error::Result;
use zbus_xml::Node;

const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

pub async fn introspect(conn: &zbus::Connection, service: &str, object: &str) -> Result<Node<'static>> {
    let proxy = zbus::Proxy::new(conn, service, object, INTROSPECTABLE).await?;
    let xml: String = proxy.call_method("Introspect", &()).await?.body().deserialize()?;
    Ok(Node::from_reader(xml.as_bytes())?)
}
```

> Note: `Node::from_reader` reads into owned buffers (quick-xml), so the `'a` lifetime is unconstrained and the result is usable as `Node<'static>`. If a future zbus_xml ties it to a borrow, parse via `Node::try_from(&*Box::leak(xml.into_boxed_str()))` instead.

- [ ] **Step 2: Rewrite `ops/introspect.rs` to render from `zbus_xml::Node`, preserving v1 output**

`src/ops/introspect.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx introspect` — thin wrapper over the async core. Renders `zbus_xml::Node`
//! into the SAME JSON/human shapes as before (spec §6), so e2e output is unchanged.

use crate::dbus;
use crate::error::Result;
use serde_json::{Value as Json, json};
use zbus_xml::{Arg, ArgDirection, Interface, Node, PropertyAccess};

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: Option<&str>,
) -> Result<()> {
    let node = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::introspect::introspect(&conn, service, object).await
    })?;

    // Optional interface filter: keep only the named interface (still an array).
    let interfaces: Vec<&Interface> = node.interfaces().iter().collect();
    let interfaces: Vec<&Interface> = match interface {
        Some(filter) => interfaces.into_iter().filter(|i| i.name().as_ref() == filter).collect(),
        None => interfaces,
    };

    if json {
        let arr: Vec<Json> = interfaces.iter().map(|i| iface_to_json(i)).collect();
        crate::out::print_json(&json!(arr));
    } else {
        print_human(&interfaces);
    }
    Ok(())
}

fn iface_to_json(iface: &Interface) -> Json {
    let methods: Vec<Json> = iface
        .methods()
        .iter()
        .map(|m| {
            json!({
                "name": m.name().to_string(),
                "in": m.args().iter().filter(|a| a.direction() == Some(ArgDirection::In)).map(arg_to_json).collect::<Vec<_>>(),
                "out": m.args().iter().filter(|a| a.direction() == Some(ArgDirection::Out)).map(arg_to_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    let signals: Vec<Json> = iface
        .signals()
        .iter()
        .map(|s| json!({ "name": s.name().to_string(), "args": s.args().iter().map(arg_to_json).collect::<Vec<_>>() }))
        .collect();
    let props: Vec<Json> = iface
        .properties()
        .iter()
        .map(|p| json!({ "name": p.name().to_string(), "type": p.ty().to_string(), "access": access_str(p.access()) }))
        .collect();
    json!({ "name": iface.name().to_string(), "methods": methods, "signals": signals, "properties": props })
}

fn arg_to_json(a: &Arg) -> Json {
    json!({ "name": a.name(), "type": a.ty().to_string() })
}

fn access_str(a: PropertyAccess) -> &'static str {
    match a {
        PropertyAccess::Read => "read",
        PropertyAccess::Write => "write",
        PropertyAccess::ReadWrite => "readwrite",
    }
}

fn print_human(interfaces: &[&Interface]) {
    for iface in interfaces {
        println!("{}", iface.name());
        for m in iface.methods() {
            let in_sig: String = m.args().iter().filter(|a| a.direction() == Some(ArgDirection::In)).map(|a| a.ty().to_string()).collect();
            let out_sig: String = m.args().iter().filter(|a| a.direction() == Some(ArgDirection::Out)).map(|a| a.ty().to_string()).collect();
            let sig = match (in_sig.is_empty(), out_sig.is_empty()) {
                (false, false) => format!("{in_sig} → {out_sig}"),
                (true, false) => format!("→ {out_sig}"),
                (false, true) => in_sig,
                (true, true) => String::new(),
            };
            println!("  .{:<16} method   {sig}", m.name());
        }
        for s in iface.signals() {
            let args: String = s.args().iter().map(|a| a.ty().to_string()).collect();
            println!("  .{:<16} signal   {args}", s.name());
        }
        for p in iface.properties() {
            println!("  .{:<16} prop     {} [{}]", p.name(), p.ty(), access_str(p.access()));
        }
    }
}
```

> `Interface::name()` returns `InterfaceName<'_>` (a `zbus_names` newtype); `.as_ref()` yields `&str` for the equality compare, and `Display` prints the plain name. `m.name()`/`s.name()` are `MemberName`, `p.name()` is `PropertyName` — all `Display` the plain string. `p.ty()` is `&CompleteType`, which `Display`s as its signature string.

- [ ] **Step 3: Delete the old roxmltree parser**

Delete `src/introspect.rs`, and remove the `mod introspect;` line from `src/main.rs`.

- [ ] **Step 4: Run the introspect e2e**

Run: `cargo test --test introspect`
Expected: PASS — JSON array with `methods[].name == "BumpVolume"` and `properties[].name == "volume"`; human output contains the interface header, `BumpVolume`, `volume`, `method`, `prop`.

> If a shape mismatch fails a test: the v1 shape is `{name, methods:[{name,in,out}], signals:[{name,args}], properties:[{name,type,access}]}` with `in`/`out` arrays of `{name,type}`. Adjust the mapping in `ops/introspect.rs` until green — do not edit the test.

- [ ] **Step 5: Commit**

```bash
git add src/dbus/introspect.rs src/ops/introspect.rs src/main.rs
git rm src/introspect.rs
git commit -m "refactor(busx): migrate introspect to zbus_xml; drop src/introspect.rs

Assisted-by: claude:glm-5.2"
```

---

## Task 6: `tree` core (zbus_xml) + migrate `ops/tree.rs`

**Files:**
- Modify: `src/dbus/tree.rs`, `src/ops/tree.rs`

- [ ] **Step 1: Implement the core `object_tree` (recursive walk via zbus_xml)**

`src/dbus/tree.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `object_tree` — recursively introspect a service's object paths (spec §6).

use crate::dbus::types::ObjectNode;
use crate::error::Result;
use zbus_xml::Node;

const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

pub async fn object_tree(conn: &zbus::Connection, service: &str) -> Result<ObjectNode> {
    let mut root = ObjectNode { path: "/".to_string(), children: vec![] };
    walk(conn, service, "/", &mut root.children).await?;
    Ok(root)
}

async fn walk(conn: &zbus::Connection, service: &str, path: &str, out: &mut Vec<ObjectNode>) -> Result<()> {
    let proxy = zbus::Proxy::new(conn, service, path, INTROSPECTABLE).await?;
    let xml: String = proxy.call_method("Introspect", &()).await?.body().deserialize()?;
    let node = Node::from_reader(xml.as_bytes())?;
    for child in node.nodes() {
        let Some(name) = child.name() else { continue };
        if name.starts_with('/') {
            // External subtree reference — following it would re-root the walk.
            continue;
        }
        let child_path = format!("{}/{}", path.trim_end_matches('/'), name);
        let mut child_node = ObjectNode { path: child_path.clone(), children: vec![] };
        // Recurse before pushing so the borrow on `out` is released each iteration.
        walk(conn, service, &child_path, &mut child_node.children).await?;
        out.push(child_node);
    }
    Ok(())
}
```

- [ ] **Step 2: Migrate `ops/tree.rs` to flatten the `ObjectNode` and render**

`src/ops/tree.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx tree SERVICE` — thin wrapper. Flattens the core's `ObjectNode` into the
//! sorted path list and prints it (human tree / JSON `{service: [paths]}`).

use crate::dbus;
use crate::dbus::types::ObjectNode;
use crate::error::Result;
use serde_json::json;

pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
) -> Result<()> {
    let root = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::tree::object_tree(&conn, service).await
    })?;

    let mut paths = Vec::new();
    flatten(&root, &mut paths);
    paths.sort();

    if json {
        let mut tree = serde_json::Map::new();
        tree.insert(service.to_string(), json!(paths));
        crate::out::print_json(&json!(tree));
    } else {
        println!("{service}");
        for p in &paths {
            println!("└─ {p}");
        }
    }
    Ok(())
}

fn flatten(node: &ObjectNode, out: &mut Vec<String>) {
    out.push(node.path.clone());
    for c in &node.children {
        flatten(c, out);
    }
}
```

- [ ] **Step 3: Run the tree e2e**

Run: `cargo test --test tree`
Expected: PASS — JSON `{org.busx.Test: ["/", "/org/busx/Test", "/org/busx/Test/sub", ...]}`; human shows the service header and both paths.

- [ ] **Step 4: Commit**

```bash
git add src/dbus/tree.rs src/ops/tree.rs
git commit -m "refactor(busx): migrate tree to zbus_xml async core

Assisted-by: claude:glm-5.2"
```

---

## Task 7: `call` core + migrate `ops/call.rs`

**Files:**
- Modify: `src/dbus/call.rs`, `src/ops/call.rs`

- [ ] **Step 1: Implement the core `call_method` returning owned values**

`src/dbus/call.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `call_method` — encode busctl-style args, invoke via a generic proxy, return
//! the reply values as owned (spec §7). Encoding is shared (`value::encode`).

use crate::error::Result;
use zvariant::{OwnedValue, Structure, StructureBuilder};

pub async fn call_method(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    signature: &str,
    args: &[String],
) -> Result<Vec<OwnedValue>> {
    let proxy = zbus::Proxy::new(conn, service, object, interface).await?;
    let values = crate::value::encode::parse(signature, args)?;

    // Wrap positional args in a Structure (carries the concatenated signature so
    // the peer sees N positional args). Empty-arg case handled separately.
    let reply = if values.is_empty() {
        proxy.call_method(method, &()).await?
    } else {
        let mut builder = StructureBuilder::new();
        for v in values {
            builder = builder.append_field(v);
        }
        proxy.call_method(method, &builder.build()?).await?
    };

    // Deserialize the reply body as a Structure (accepts any signature; one value
    // wraps in a single-field struct) → owned values.
    let body = reply.body();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let structure: Structure = body.deserialize()?;
    Ok(structure
        .fields()
        .iter()
        .map(|f| f.try_to_owned())
        .collect::<std::result::Result<_, _>>()?)
}
```

> `Value::try_to_owned() -> Result<OwnedValue>` is the zvariant owned conversion. If the resolved zvariant names it differently (e.g. `to_owned`), use that — the `call`/`get` e2e tests guard the rendered output.

- [ ] **Step 2: Migrate `ops/call.rs`**

`src/ops/call.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx call` — thin wrapper: run the async core, render return values (human
//! `<sig>  <pretty>` per line / type-tagged JSON array). spec §7.

use crate::dbus;
use crate::error::Result;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    signature: &str,
    args: &[String],
) -> Result<()> {
    let fields = async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::call::call_method(&conn, service, object, interface, method, signature, args).await
    })?;

    if json {
        let out: Vec<_> = fields.iter().map(|f| crate::value::decode::to_tagged(f)).collect();
        crate::out::print_json(&json!(out));
    } else {
        for f in &fields {
            println!("{}  {}", f.value_signature(), crate::value::pretty::pretty(f));
        }
    }
    Ok(())
}
```

> `&OwnedValue` derefs to `&Value`, so `to_tagged(f)` / `pretty(f)` / `f.value_signature()` work unchanged.

- [ ] **Step 3: Run the call e2e**

Run: `cargo test --test call`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/dbus/call.rs src/ops/call.rs
git commit -m "refactor(busx): migrate call onto async dbus core

Assisted-by: claude:glm-5.2"
```

---

## Task 8: `property` core + migrate `ops/property.rs`

**Files:**
- Modify: `src/dbus/property.rs`, `src/ops/property.rs`

- [ ] **Step 1: Implement the core `get_all` / `get_one` / `set`**

`src/dbus/property.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property read/write via `org.freedesktop.DBus.Properties` (spec §8). Owned
//! values so callers can store them.

use crate::error::{Error, Result};
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zvariant::OwnedValue;

pub async fn get_all(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
) -> Result<Vec<(String, OwnedValue)>> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let name = InterfaceName::try_from(iface)?;
    let map = proxy.get_all(name).await?;
    Ok(map.into_iter().map(|(k, v)| (k, OwnedValue::from(v))).collect())
}

pub async fn get_one(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
    prop: &str,
) -> Result<OwnedValue> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let name = InterfaceName::try_from(iface)?;
    Ok(OwnedValue::from(proxy.get(name, prop).await?))
}

pub async fn set(
    conn: &zbus::Connection,
    service: &str,
    object: &str,
    iface: &str,
    prop: &str,
    signature: &str,
    value_tokens: &[String],
) -> Result<()> {
    let proxy = PropertiesProxy::new(conn, service, object).await?;
    let mut parsed = crate::value::encode::parse(signature, value_tokens)?;
    let value = parsed.pop().ok_or_else(|| Error::Msg("set: missing value".into()))?;
    if !parsed.is_empty() {
        return Err(Error::Msg("set: expected exactly one value".into()));
    }
    let name = InterfaceName::try_from(iface)?;
    proxy.set(name, prop, &value).await?;
    Ok(())
}
```

> `OwnedValue::from(Value)` converts a borrowed value to owned. If that `From` impl isn't present in the resolved version, use the same owned-conversion as Task 7 (`try_to_owned`).

- [ ] **Step 2: Migrate `ops/property.rs`**

`src/ops/property.rs` (replaces the whole file; preserves v1 `get`/`set` overload behaviour):

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx get` / `busx set` — thin wrappers over the async core. `get` overloads:
//! no names ⇒ GetAll; names ⇒ per-name Get (spec §8).

use crate::dbus;
use crate::error::{Error, Result};
use serde_json::{Map, Value as Json, json};
use zbus::names::InterfaceName;

#[allow(clippy::too_many_arguments)]
pub fn get(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: Option<&str>,
    props: &[String],
) -> Result<()> {
    let get_all_only = props.is_empty();
    match interface {
        None | Some("") if get_all_only => {
            let map = async_global_executor::block_on(async {
                let conn = dbus::conn::connect(user, system, address, verbose).await?;
                dbus::property::get_all(&conn, service, object, "").await
            })?;
            print_map(&map, json)
        }
        None | Some("") => Err(Error::Msg(
            "get: --interface is required when reading individual properties".into(),
        )),
        Some(name) => {
            let _ = InterfaceName::try_from(name).map_err(zbus::Error::from)?;
            if get_all_only {
                let map = async_global_executor::block_on(async {
                    let conn = dbus::conn::connect(user, system, address, verbose).await?;
                    dbus::property::get_all(&conn, service, object, name).await
                })?;
                print_map(&map, json)
            } else {
                let values = async_global_executor::block_on(async {
                    let conn = dbus::conn::connect(user, system, address, verbose).await?;
                    let mut vs = Vec::with_capacity(props.len());
                    for p in props {
                        vs.push(dbus::property::get_one(&conn, service, object, name, p).await?);
                    }
                    Ok::<_, Error>(vs)
                })?;
                if json {
                    let arr: Vec<_> = values.iter().map(|v| crate::value::decode::to_tagged(v)).collect();
                    crate::out::print_json(&json!(arr));
                } else {
                    for v in &values {
                        println!("{}  {}", v.value_signature(), crate::value::pretty::pretty(v));
                    }
                }
                Ok(())
            }
        }
    }
}

fn print_map(map: &[(String, zvariant::OwnedValue)], json: bool) -> Result<()> {
    if json {
        let mut obj = Map::new();
        for (k, v) in map {
            obj.insert(k.clone(), crate::value::decode::to_tagged(v));
        }
        crate::out::print_json(&Json::Object(obj));
    } else {
        let mut names: Vec<&String> = map.iter().map(|(k, _)| k).collect();
        names.sort();
        for k in names {
            let (_, v) = map.iter().find(|(kk, _)| kk == k).unwrap();
            println!("{}  {}  {}", k, v.value_signature(), crate::value::pretty::pretty(v));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn set(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    service: &str,
    object: &str,
    interface: &str,
    property: &str,
    signature: &str,
    value: &[String],
) -> Result<()> {
    async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address, verbose).await?;
        dbus::property::set(&conn, service, object, interface, property, signature, value).await
    })
}
```

> Note: `get_all` for all-interfaces uses the empty interface name `""` (injected unchecked, matching dbus-send/busctl). The core's `get_all(.., "")` passes it through; `InterfaceName::try_from("")` would reject it, so for that call the core must inject the empty name without validation (as v1 does via `from_str_unchecked`). If `InterfaceName::try_from("")` in the core errors, change the core's `get_all` to use `InterfaceName::from_str_unchecked("")` for the all-interfaces case — the e2e `get` test for `GetAll("")` guards this.

- [ ] **Step 3: Run the get/set e2e**

Run: `cargo test --test get && cargo test --test set`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/dbus/property.rs src/ops/property.rs
git commit -m "refactor(busx): migrate get/set onto async dbus core

Assisted-by: claude:glm-5.2"
```

---

## Task 9: Extract `build_match_rule` into `dbus/monitor.rs`

**Files:**
- Modify: `src/dbus/monitor.rs`, `src/ops/monitor.rs`

The blocking monitor stream stays as-is; only the pure match-rule builder moves to the shared core so the future TUI monitor can reuse it.

- [ ] **Step 1: Move `build_match_rule` (verbatim) into the core**

`src/dbus/monitor.rs` — move the existing `build_match_rule` function from `src/ops/monitor.rs` here unchanged (it's already pure: takes `Option<&str>` filters and a `signals` bool, returns `Result<MatchRule<'static>>`). Add the SPDX header and a doc comment. Keep `use` of `crate::error::{Error, Result}` and `zbus::MatchRule`.

- [ ] **Step 2: Update `ops/monitor.rs` to call the core's `build_match_rule`**

In `src/ops/monitor.rs`:
- Remove the local `build_match_rule` fn.
- Replace its call site (`let rule = build_match_rule(...)?;`) with `let rule = crate::dbus::monitor::build_match_rule(...)?;`.

Leave everything else in `ops/monitor.rs` (the blocking `MessageIterator` stream, `become_monitor`, `msg_to_json`, `msg_to_human`, `stream`, `matches_service`, `parse_duration`, `flags_of`, `epoch_secs`) unchanged — it still uses the blocking `crate::conn::connect` for its stream.

- [ ] **Step 3: Run the monitor e2e**

Run: `cargo test --test monitor`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/dbus/monitor.rs src/ops/monitor.rs
git commit -m "refactor(busx): extract build_match_rule to dbus core

Assisted-by: claude:glm-5.2"
```

---

## Task 10: Drop `roxmltree`; full-suite green

**Files:**
- Modify: `Cargo.toml`

After Tasks 5–6, nothing uses `roxmltree` (introspect/tree moved to `zbus_xml`).

- [ ] **Step 1: Remove the dependency**

In `Cargo.toml`, delete the `roxmltree = "0.21"` line.

- [ ] **Step 2: Verify no roxmltree usage remains**

Run: `rg -n 'roxmltree' src/ tests/ || true`
Expected: no matches.

- [ ] **Step 3: Build + clippy + fmt + full e2e**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo test`
Expected: all green. (Every existing CLI e2e test must pass — this is the contract that the refactor preserved behaviour.)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "refactor(busx): drop roxmltree (introspection now via zbus_xml)

Phase 0 complete: async dbus core + zbus_xml, all CLI e2e green.

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist (run before handing off)

- **Spec coverage:** list ✓ (T4), introspect ✓ (T5), tree ✓ (T6), call ✓ (T7), get/set ✓ (T8), shared async core ✓ (T1–T3), zbus_xml adoption ✓ (T5–T6), build_match_rule extraction ✓ (T9), roxmltree removal ✓ (T10). Monitor's full async migration + `src/conn.rs` removal → deferred to the monitor phase (documented above). BusMessage type → deferred (not needed until the monitor phase).
- **Type consistency:** `ServiceInfo`/`ObjectNode` defined in T2, used in T4/T6. Core fns return `Vec<OwnedValue>` (T7/T8); ops consume via deref. `build_match_rule` signature unchanged in T9.
- **No placeholders:** every step has concrete code or exact commands; the few "confirm against the compiler" notes name the specific API to verify (zvariant owned conversion, `CompleteType`/name-type `Display`, empty-interface `GetAll`), with the e2e test as the guard.
- **Behaviour-preserving:** output shapes are explicitly kept identical; the whole `tests/` suite is the gate in T10.

---

## Roadmap — follow-up plans (phases 1–6)

Each becomes its own plan, written after the prior phase lands against real code. Outlines only — not executable detail:

1. **TUI skeleton + Service page** — deps `ratatui`, `crossterm`, `tui-tree-widget`, `tui-input`, `arboard`, `flume`, `futures`; `command: Option<Command>` (bare `busx` ⇒ TUI); terminal setup/teardown; crossterm event loop; `flume` channel; Elm `state`/`update`/`render`; `ratatui TestBackend` harness; Service list page driven by `dbus::list::list_names`.
2. **Browse flow** — Objects (`tui-tree-widget`) → Interfaces (exclude `org.freedesktop.DBus.{Introspectable,Properties,Peer}`) → Interface (3 columns; property `GetAll` snapshot + `r` refresh); single-item auto-skip; breadcrumb + bottom key-hint bar.
3. **Call / read / write** — method-call detail (per-arg `tui-input`) + result; property get/set detail + result; one-shot ops with loading state.
4. **Listen + cancel** — signal listen / property watch (match-rule on shared connection) / method listen (dedicated connection + `BecomeMonitor`); result streaming; `Esc`-leaves-stops via `select!` + dropped cancel sender. *(This phase also migrates `ops/monitor.rs` to the async core and removes `src/conn.rs`.)*
5. **copy-as + clipboard** — `tui/copy.rs` generating dbus-send/busctl/qdbus/gdbus; `arboard` copy; copy-as popup preview.
6. **Polish** — `?` help overlay, error toasts, JSON view toggle, empty/edge states, TestBackend snapshot coverage.
