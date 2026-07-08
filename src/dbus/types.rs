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
