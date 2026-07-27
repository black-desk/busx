// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned data types returned by the async core. Most introspection results
//! reuse `zbus_xml` types directly; the busx-owned types here are the
//! exceptions: `ServiceInfo` (name + enrichment from `list`) and `ObjectNode`
//! (the recursive-introspection tree `busx tree` / the TUI build).

/// One bus name with best-effort PID + process enrichment.
pub struct ServiceInfo {
    pub name: String,
    pub pid: Option<u64>,
    pub process: Option<String>,
}

/// A node in an object-path tree (the result of recursively introspecting a
/// service). `path` is the absolute object path; `children` are sub-objects;
/// `interfaces` are the names of the interfaces this object exposes (empty ⇒ a
/// pure container path with no object of its own — filtered from the flat TUI
/// view).
pub struct ObjectNode {
    pub path: String,
    pub interfaces: Vec<String>,
    pub children: Vec<ObjectNode>,
}
