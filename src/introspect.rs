// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Introspection XML → JSON parsing (spec §6).
//!
//! `org.freedesktop.DBus.Introspectable.Introspect` returns an XML description
//! of an object's interfaces; this module turns it into a JSON array of
//! `{ name, methods, signals, properties }` interface objects. `roxmltree` is a
//! zero-copy streaming parser, well suited to this small, shallow DOM.

use serde_json::{Value as Json, json};

/// Parse introspection XML into a JSON array of interface objects.
///
/// On a parse failure the function returns a single `{ "error": ... }` object
/// rather than panicking, so the op layer can still emit valid JSON.
pub fn parse_xml(xml: &str) -> Json {
    // zbus's introspection XML opens with a `<!DOCTYPE node PUBLIC ...>` doctype,
    // so the default `allow_dtd = false` rejects it. The DTD here is a fixed
    // declaration (no external entity expansion), so enabling it is safe.
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = match roxmltree::Document::parse_with_options(xml, opts) {
        Ok(d) => d,
        Err(e) => return json!({ "error": format!("parse introspection XML: {e}") }),
    };

    // A zbus introspection XML document is *recursive*: a registered child
    // object is reported as a `<node name="...">` element whose own subtree
    // repeats the child's interfaces. The interfaces of *this* object are only
    // the `<interface>` elements that are direct children of the root `<node>`,
    // so iterate those rather than every interface in the document (which would
    // double-count interfaces also exposed by registered sub-objects).
    let root = doc.root_element();
    let mut ifaces = Vec::new();
    for iface in root.children().filter(|n| n.has_tag_name("interface")) {
        let mut methods = Vec::new();
        let mut signals = Vec::new();
        let mut props = Vec::new();
        for child in iface.children() {
            if child.has_tag_name("method") {
                methods.push(json!({
                    "name": child.attribute("name"),
                    "in": args_of(&child, "in"),
                    "out": args_of(&child, "out"),
                }));
            } else if child.has_tag_name("signal") {
                signals.push(json!({
                    "name": child.attribute("name"),
                    "args": args_of(&child, "arg"),
                }));
            } else if child.has_tag_name("property") {
                props.push(json!({
                    "name": child.attribute("name"),
                    "type": child.attribute("type"),
                    "access": child.attribute("access"),
                }));
            }
        }
        ifaces.push(json!({
            "name": iface.attribute("name"),
            "methods": methods,
            "signals": signals,
            "properties": props,
        }));
    }
    json!(ifaces)
}

/// Collect the `<arg>` children of `node`.
///
/// For methods, args carry a `direction` of `in`/`out` — pass that direction.
/// For signals, args have no `direction` attribute, so callers pass `"arg"` to
/// mean "no direction filter" (the `None` arm matches only when `dir == "arg"`).
fn args_of(node: &roxmltree::Node, dir: &str) -> Vec<Json> {
    node.children()
        .filter(|n| n.has_tag_name("arg"))
        .filter(|a| match a.attribute("direction") {
            Some(d) => d == dir,
            None => dir == "arg",
        })
        .map(|a| {
            json!({
                "name": a.attribute("name"),
                "type": a.attribute("type"),
            })
        })
        .collect()
}
