// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shell completion via `clap_complete::dynamic`.
//!
//! Two layers:
//! - **Structural completion** (subcommand names, flag names, global-flag
//!   parsing) is delegated to clap by `CompleteEnv` + the `complete` engine,
//!   driven off the real `Cli::command()` (no hand-built mirror). The shell
//!   re-invokes `busx` with the full command line under a special env var
//!   (`COMPLETE=<shell>`); `try_complete` processes it and exits.
//! - **Positional values** (service / object-path / interface / method /
//!   signature / property) get
//!   live bus introspection via `ArgValueCompleter` closures attached to each
//!   positional in `cli.rs` with `#[arg(add = ...)]`. The closures (exported
//!   below as `pub fn`) read the bus flags and the already-typed positionals
//!   straight from `std::env::args_os()` — the same arg vector clap itself
//!   parses — so completion connects to the bus the user actually selected
//!   (`--user`/`--system`/`--address`).
//!
//! Everything here is best-effort: a bus error yields no candidates (and the
//! command never fails), and introspection is **deliberately uncached**
//! (re-issued each TAB). The shell invokes each `ArgValueCompleter` closure as
//! a fresh, stateless subprocess on every TAB, so any cache would live in a
//! process that exits between keystrokes. To persist it you'd need an external
//! cache (file/daemon) plus a TTL/invalidation policy — and then the
//! per-invocation closures, which are intentionally trivial (read args,
//! introspect, filter), would each have to reason about cache freshness,
//! keying, and staleness. Stale candidates (a service that just quit, an object
//! path added since) are worse than a fresh introspect, and the bus is the
//! local `dbus-daemon`, so the round-trip is cheap. Simpler + correct to
//! introspect every time.

use std::ffi::{OsStr, OsString};

use clap::CommandFactory;
use clap_complete::{CompleteEnv, CompletionCandidate, Shell};
use zbus::blocking::Connection;
use zbus::blocking::fdo::DBusProxy;
use zbus_xml::{ArgDirection, Node};

use crate::error::Result;

/// Entry point invoked very early in `main`. If `COMPLETE=<shell>` is set the
/// shell is asking us to produce candidates (or the registration script); we do
/// so, write them to stdout, and the caller exits `0`. Otherwise this is a
/// normal run and we return `Ok(false)` so `main` proceeds to parse args.
pub fn try_complete() -> Result<bool> {
    let current_dir = std::env::current_dir().ok();
    CompleteEnv::with_factory(crate::cli::Cli::command)
        .try_complete(std::env::args_os(), current_dir.as_deref())
        .map_err(|e| crate::error::Error::Msg(format!("completion: {e}")))
}

/// Emit the `clap_complete::dynamic` registration script for `shell`. This is
/// the "source me" form — e.g. for bash, `source <(busx completion bash)`.
///
/// `CompleteEnv` writes the registration script to stdout when invoked with
/// only the binary name (no `-- <words>`). We reproduce that by setting the env
/// var and re-running the try_complete path with a bare argv: it prints the
/// registration script for the requested shell.
pub fn emit_script(shell: Shell) {
    if let Some(name) = shell_name(shell) {
        // SAFETY: completion registration runs at process start, before any
        // threads exist.
        unsafe { std::env::set_var("COMPLETE", name) };
        let bin = std::env::args_os().next().unwrap_or_else(|| "busx".into());
        let current_dir = std::env::current_dir().ok();
        let _ = CompleteEnv::with_factory(crate::cli::Cli::command)
            .try_complete([bin], current_dir.as_deref());
    }
}

/// Map the AOT `Shell` enum to the dynamic completer's shell name (the same
/// string the `COMPLETE=` env var accepts). Returns `None` for shells the
/// dynamic engine doesn't ship a registration script for.
fn shell_name(shell: Shell) -> Option<&'static str> {
    match shell {
        Shell::Bash => Some("bash"),
        Shell::Elvish => Some("elvish"),
        Shell::Fish => Some("fish"),
        Shell::PowerShell => Some("powershell"),
        Shell::Zsh => Some("zsh"),
        _ => None,
    }
}

/// The "kind" of bus value a positional holds. Used by `complete_positional`
/// to dispatch to the matching introspection helper. Positionals with no
/// completion (e.g. method args, property values) simply have no
/// `ArgValueCompleter` attached in `cli.rs`.
#[derive(Clone, Copy)]
enum Kind {
    Service,
    Path,
    Interface,
    Method,
    Signature,
    Property,
}

/// Completer fn for the service positional. `pub` so `cli.rs`'s
/// `#[arg(add = ArgValueCompleter::new(complete::complete_service))]` can name it.
pub fn complete_service(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Service, current)
}

/// Completer fn for the object-path positional.
pub fn complete_path(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Path, current)
}

/// Completer fn for the interface positional.
pub fn complete_interface(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Interface, current)
}

/// Completer fn for the method positional.
pub fn complete_method(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Method, current)
}

/// Completer fn for the signature positional of `call`. Returns the method's
/// input signature (a single candidate), filtered by the partial token.
pub fn complete_signature(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Signature, current)
}

/// Completer fn for the property-name positional(s) of `get`/`set`. Lists the
/// property names of the chosen (or all) interface(s) on the object.
pub fn complete_property(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Property, current)
}

/// The per-positional dynamic completer. Reads the bus flags + filled
/// positionals from `std::env::args_os()` (the same vector clap parses),
/// connects to the resulting bus, and dispatches to the matching introspection
/// helper. `current` is the partial token the user is typing.
fn complete_positional(kind: Kind, current: &OsStr) -> Vec<CompletionCandidate> {
    let parsed = parse_args();
    let Some(sub) = parsed.subcommand else {
        return Vec::new();
    };
    let current = match current.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    // Connect via the async `dbus::conn::connect` (the blocking `crate::conn`
    // wrapper is gone); block on it here because the clap_complete completer
    // closure is synchronous. The introspection helpers below still use the
    // blocking proxy API, so convert the async connection back to a
    // `zbus::blocking::Connection` (zero-cost wrap — the blocking API is just
    // `block_on` over the async core).
    let conn = match async_global_executor::block_on(crate::dbus::conn::connect(
        parsed.user,
        parsed.system,
        parsed.address.as_deref(),
    )) {
        Ok(c) => zbus::blocking::Connection::from(c),
        Err(_) => return Vec::new(),
    };
    let cands =
        positional_candidates(&conn, &sub, &parsed.positionals, kind, current).unwrap_or_default();
    cands.into_iter().map(CompletionCandidate::new).collect()
}

/// Decoded view of the raw argv relevant to completion: the bus flags, the
/// subcommand, and the already-filled positional values (the partial being typed
/// is excluded — it arrives separately as `current`).
///
/// This is parsed by the **real** [`crate::cli::Cli::command()`] with
/// [`ignore_errors(true)`][clap::Command::ignore_errors]: clap is the single
/// source of truth for the flag/positional layout, so there is no hand-written
/// argv walker to keep in sync when the CLI changes. `ignore_errors` lets clap
/// accept the half-typed command line completion always sees (the final token
/// may be a partial value); clap still correctly separates options from
/// positionals — exactly what the old walker hand-coded and what a plain
/// `get_matches` would reject mid-word.
struct ParsedArgs {
    user: bool,
    system: bool,
    address: Option<String>,
    subcommand: Option<String>,
    positionals: Vec<String>,
}

/// Parse the real CLI definition, tolerating the incomplete tail that
/// completion always produces. clap separates options from positionals and
/// resolves the active subcommand — no hand-maintained flag list, no hand-coded
/// subcommand-name table. The subcommand's filled positionals are collected in
/// index order, then the last is dropped because clap parsed the partial being
/// typed as a positional and it arrives separately as `current`.
fn parse_args() -> ParsedArgs {
    // The completion protocol re-invokes us as `busx -- <words...>`. That
    // leading `--` ends option parsing, so it must be dropped before the
    // argv is handed to clap — otherwise clap reads every word as a
    // positional literal of the top-level command and never resolves the
    // subcommand. This mirrors what `CompleteEnv::try_complete_` drains
    // before calling the engine; the per-positional closures read
    // `args_os()` directly, so they repeat the same trim. `<words>` still
    // starts with the program name, which clap consumes as the binary name.
    let args: Vec<OsString> = std::env::args_os().collect();
    let escape = args
        .iter()
        .position(|a| a.as_os_str() == OsStr::new("--"))
        .map(|i| i + 1)
        .unwrap_or(1);

    // Tolerate the incomplete tail completion always feeds us.
    let matches = crate::cli::Cli::command()
        .ignore_errors(true)
        .try_get_matches_from(args.into_iter().skip(escape));
    let Ok(matches) = matches else {
        return ParsedArgs {
            user: false,
            system: false,
            address: None,
            subcommand: None,
            positionals: Vec::new(),
        };
    };

    let user = matches.get_flag("user");
    let system = matches.get_flag("system");
    let address = matches.get_one::<String>("address").cloned();

    // Collect the active subcommand's positionals in index order, then drop the
    // last — clap parsed the partial being typed as a positional.
    let (subcommand, positionals) = match matches.subcommand_name() {
        Some(name) => {
            let pos: Vec<String> = match matches.subcommand_matches(name) {
                Some(sm) => {
                    // Build a fresh Command purely to reflect the subcommand's
                    // positional *layout* — the parsed values come from `sm`,
                    // not from this Command. `Arg::get_index()` returns `None`
                    // until the Command is built: clap assigns positional
                    // indices during `build()`, so without it every positional
                    // is filtered out and we'd collect nothing. The Command is
                    // held on the stack so the borrowed arg ids outlive the
                    // `pos_ids` collect below.
                    let mut cmd = crate::cli::Cli::command();
                    cmd.build();
                    let mut pos_ids: Vec<(usize, &str)> = cmd
                        .find_subcommand(name)
                        .iter()
                        .flat_map(|sc| sc.get_arguments())
                        .filter_map(|a| a.get_index().map(|i| (i, a.get_id().as_str())))
                        .collect();
                    pos_ids.sort_by_key(|(i, _)| *i);
                    let mut v: Vec<String> = pos_ids
                        .into_iter()
                        .filter_map(|(_, id)| sm.get_one::<String>(id).cloned())
                        .collect();
                    if !v.is_empty() {
                        v.pop();
                    }
                    v
                }
                None => Vec::new(),
            };
            (Some(name.to_string()), pos)
        }
        None => (None, Vec::new()),
    };

    ParsedArgs {
        user,
        system,
        address,
        subcommand,
        positionals,
    }
}

/// Dispatch to the introspection helper for `kind` using the filled positionals.
/// `positionals` excludes the partial. Mirrors the position semantics documented
/// in the spec (call: svc/obj/iface/method; get: svc/obj/iface?; etc.).
fn positional_candidates(
    conn: &Connection,
    sub: &str,
    positionals: &[String],
    kind: Kind,
    partial: &str,
) -> Result<Vec<String>> {
    let nth = |i: usize| positionals.get(i).map(|s| s.as_str()).unwrap_or("");
    match (sub, kind) {
        (_, Kind::Service) => service_names(conn, partial),
        ("call" | "get" | "set" | "introspect", Kind::Path) => child_paths(conn, nth(0), partial),
        ("call" | "get" | "set" | "introspect", Kind::Interface) => {
            interface_names(conn, nth(0), nth(1), partial)
        }
        ("call", Kind::Method) => method_names(conn, nth(0), nth(1), nth(2), partial),
        ("call", Kind::Signature) => {
            method_input_signature_candidates(conn, nth(0), nth(1), nth(2), nth(3), partial)
        }
        // `get`'s property positional is variadic: every position from index 3
        // onward (after service/object/[interface]) completes property names.
        // `filled[2]` is the interface the user typed (possibly empty for `get`,
        // where it's optional) — empty ⇒ all interfaces of the object.
        ("get", Kind::Property) => property_names(conn, nth(0), nth(1), nth(2), partial),
        // `set`'s single property positional sits at index 3.
        ("set", Kind::Property) => property_names(conn, nth(0), nth(1), nth(2), partial),
        _ => Ok(Vec::new()),
    }
}

// --- live bus introspection helpers (best-effort, uncached) -----------------

/// Candidate services: well-known (non-unique) names on the bus, filtered to
/// those that start with the partial token.
fn service_names(conn: &Connection, partial: &str) -> Result<Vec<String>> {
    let dbus = DBusProxy::new(conn)?;
    let mut names: Vec<String> = dbus
        .list_names()?
        .into_iter()
        .filter(|n| !n.starts_with(':'))
        .map(|n| n.to_string())
        .filter(|n| n.starts_with(partial))
        .collect();
    names.sort();
    Ok(names)
}

/// Candidate object paths: introspect `/` on `service`, emit each immediate
/// child as a full path (`/<name>`), filtered by the partial token. Only one
/// level is expanded — the shell re-invokes completion for the next segment.
fn child_paths(conn: &Connection, service: &str, partial: &str) -> Result<Vec<String>> {
    let xml = introspect_xml(conn, service, "/")?;
    let mut paths: Vec<String> = parse_node_names(&xml)
        .into_iter()
        .filter(|name| !name.starts_with('/'))
        .map(|name| format!("/{name}"))
        .filter(|p| p.starts_with(partial))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Candidate interface names exposed by `service` at `object`, filtered by the
/// partial token.
fn interface_names(
    conn: &Connection,
    service: &str,
    object: &str,
    partial: &str,
) -> Result<Vec<String>> {
    let xml = introspect_xml(conn, service, object)?;
    let mut names: Vec<String> = parse_root_interface_names(&xml)
        .into_iter()
        .filter(|n| n.starts_with(partial))
        .collect();
    names.sort();
    Ok(names)
}

/// Candidate method names of `interface` on `service` at `object`, filtered by
/// the partial token.
fn method_names(
    conn: &Connection,
    service: &str,
    object: &str,
    interface: &str,
    partial: &str,
) -> Result<Vec<String>> {
    let xml = introspect_xml(conn, service, object)?;
    let mut names: Vec<String> = parse_interface_methods(&xml, interface)
        .into_iter()
        .filter(|n| n.starts_with(partial))
        .collect();
    names.sort();
    Ok(names)
}

/// Candidate property names of `interface` on `service` at `object`, filtered
/// by the partial token. If `interface` is empty, lists properties across all of
/// the object's own interfaces (de-duplicated).
fn property_names(
    conn: &Connection,
    service: &str,
    object: &str,
    interface: &str,
    partial: &str,
) -> Result<Vec<String>> {
    let xml = introspect_xml(conn, service, object)?;
    let mut names: Vec<String> = parse_interface_properties(&xml, interface)
        .into_iter()
        .filter(|n| n.starts_with(partial))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}
/// Candidate input signature of the method — a single candidate, filtered by
/// the partial token. For a no-arg method the signature is `""`, returned
/// as-is so the user can accept it.
fn method_input_signature_candidates(
    conn: &Connection,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    partial: &str,
) -> Result<Vec<String>> {
    match method_input_signature(conn, service, object, interface, method) {
        Some(sig) if sig.starts_with(partial) => Ok(vec![sig]),
        _ => Ok(Vec::new()),
    }
}

/// Best-effort: introspect `object`, find `<interface name=interface>`, find its
/// `<method name=method>`, and concatenate the `type` of every
/// `<arg direction="in">` child into one signature string. Returns `None` on any
/// error or if the method is not found.
fn method_input_signature(
    conn: &Connection,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
) -> Option<String> {
    let xml = introspect_xml(conn, service, object).ok()?;
    parse_method_input_signature(&xml, interface, method)
}

/// Call `Introspect` on `service` at `path`, returning the raw XML.
///
/// The dedicated `IntrospectableProxy` hard-codes `default_path = "/"`, so it
/// can't target an arbitrary object path — the generic `Proxy` carries the real
/// path (mirrors `src/ops/tree.rs`).
fn introspect_xml(conn: &Connection, service: &str, path: &str) -> Result<String> {
    let proxy =
        zbus::blocking::Proxy::new(conn, service, path, crate::dbus::introspect::INTROSPECTABLE)?;
    Ok(proxy.introspect()?)
}

/// Parse introspection XML into a `zbus_xml::Node`. Returns `None` on parse
/// failure so callers degrade to empty candidates. (`Node::from_reader` handles
/// the `<!DOCTYPE>` zbus ships.)
fn parse_node(xml: &str) -> Option<Node<'static>> {
    Node::from_reader(xml.as_bytes()).ok()
}

/// Parse `<node name="..."/>` child entries — the immediate children of the root
/// (only one level; the shell re-invokes completion for the next path segment).
fn parse_node_names(xml: &str) -> Vec<String> {
    let Some(node) = parse_node(xml) else {
        return Vec::new();
    };
    node.nodes()
        .iter()
        .filter_map(|c| c.name().map(|s| s.to_string()))
        .collect()
}

/// The interface names that are direct children of the root `<node>` (the
/// object's own interfaces, including the standard ones).
fn parse_root_interface_names(xml: &str) -> Vec<String> {
    let Some(node) = parse_node(xml) else {
        return Vec::new();
    };
    node.interfaces()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

/// The method names of `interface` (a direct child of the root).
fn parse_interface_methods(xml: &str, interface: &str) -> Vec<String> {
    let Some(node) = parse_node(xml) else {
        return Vec::new();
    };
    node.interfaces()
        .iter()
        .find(|i| i.name().as_ref() == interface)
        .into_iter()
        .flat_map(|iface| iface.methods().iter().map(|m| m.name().to_string()))
        .collect()
}

/// The property names of `interface` (a direct child of the root). If
/// `interface` is empty, collects from all of the root's own interfaces — useful
/// for `get` when the interface positional is omitted.
fn parse_interface_properties(xml: &str, interface: &str) -> Vec<String> {
    let Some(node) = parse_node(xml) else {
        return Vec::new();
    };
    node.interfaces()
        .iter()
        .filter(|i| interface.is_empty() || i.name().as_ref() == interface)
        .flat_map(|i| i.properties().iter().map(|p| p.name().to_string()))
        .collect()
}

/// Concatenate the signature of every in-arg of the named `<method>` in the named
/// `<interface>`. `None` if the document can't parse or the method is absent;
/// `Some("")` for a no-arg method.
fn parse_method_input_signature(xml: &str, interface: &str, method: &str) -> Option<String> {
    let node = parse_node(xml)?;
    let iface = node
        .interfaces()
        .iter()
        .find(|i| i.name().as_ref() == interface)?;
    let m = iface
        .methods()
        .iter()
        .find(|m| m.name().as_ref() == method)?;
    let sig: String = m
        .args()
        .iter()
        .filter(|a| a.direction() == Some(ArgDirection::In))
        .map(|a| a.ty().inner().to_string())
        .collect();
    Some(sig)
}
