//! Shell completion via `clap_complete::dynamic` (spec §12).
//!
//! Two layers:
//! - **Structural completion** (subcommand names, flag names, global-flag
//!   parsing) is delegated to clap by `CompleteEnv` + the `complete` engine.
//!   The shell re-invokes `busx` with the full command line under a special
//!   env var (`COMPLETE=<shell>`); `try_complete` processes it and exits.
//! - **Positional values** (service / object-path / interface / method) get
//!   live bus introspection via `ArgValueCompleter` closures attached to each
//!   positional when building the clap `Command`. The closures read the bus
//!   flags and the already-typed positionals straight from `std::env::args_os()`
//!   — the same arg vector clap itself parses — so completion connects to the
//!   bus the user actually selected (`--user`/`--system`/`--address`).
//!
//! Everything here is best-effort: a bus error yields no candidates (and the
//! command never fails), and introspection is uncached (re-issued each TAB).

use std::ffi::OsStr;

use clap::builder::ValueHint;
use clap::{Arg, ArgAction, Command};
use clap_complete::{ArgValueCompleter, CompletionCandidate, CompleteEnv, Shell};
use zbus::blocking::Connection;
use zbus::blocking::fdo::DBusProxy;

use crate::conn::connect;
use crate::error::Result;

/// The interface whose `Introspect` method we call. Every object implements it.
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Names of subcommands that take a service as their first positional.
const SERVICE_SUBS: &[&str] = &["call", "get", "set", "introspect", "tree", "monitor"];

/// Entry point invoked very early in `main`. If `COMPLETE=<shell>` is set the
/// shell is asking us to produce candidates (or the registration script); we do
/// so, write them to stdout, and the caller exits `0`. Otherwise this is a
/// normal run and we return `Ok(false)` so `main` proceeds to parse args.
pub fn try_complete() -> Result<bool> {
    let current_dir = std::env::current_dir().ok();
    CompleteEnv::with_factory(command)
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
        let _ = CompleteEnv::with_factory(command).try_complete([bin], current_dir.as_deref());
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

/// Build the `Command` mirror of `crate::cli::Cli` with dynamic completion
/// attached to the bus-walking positionals. This is a hand-built mirror rather
/// than `Cli::command()` because `ArgValueCompleter` is attached via the
/// programmatic `Arg::add` API (clap derive has no `add = ...` attribute in the
/// resolved clap version), and we only need the surface clap parses for
/// completion — not the real value semantics.
fn command() -> Command {
    let global = |name: &'static str, help: &'static str| {
        Arg::new(name).long(name).global(true).action(ArgAction::SetTrue).help(help)
    };
    Command::new("busx")
        .bin_name("busx")
        .about("D-Bus CLI (dbus-send/busctl/qdbus replacement)")
        .arg(global("user", "Connect to the session bus (fallback to system)"))
        .arg(global("system", "Connect to the system bus"))
        .arg(
            Arg::new("address")
                .long("address")
                .global(true)
                .value_name("ADDRESS")
                .action(ArgAction::Set)
                .help("Connect to the bus at ADDRESS"),
        )
        .arg(global("verbose", "Verbose diagnostics on stderr"))
        .arg(global("json", "Emit type-tagged JSON (default: human text)"))
        .subcommands([
            subcommand("list").arg(flag("unique")).arg(flag("acquired")).arg(flag("activatable")),
            subcommand("tree").arg(positional("service", Service)),
            subcommand("introspect")
                .arg(positional("service", Service))
                .arg(positional("object", Path))
                .arg(positional_opt("interface", Interface)),
            subcommand("call")
                .arg(positional("service", Service))
                .arg(positional("object", Path))
                .arg(positional("interface", Interface))
                .arg(positional("method", Method))
                .arg(positional("signature", Signature))
                .arg(positional_vec("args", None)),
            subcommand("get")
                .arg(positional("service", Service))
                .arg(positional("object", Path))
                .arg(positional_opt("interface", Interface))
                .arg(positional_vec("props", Property)),
            subcommand("set")
                .arg(positional("service", Service))
                .arg(positional("object", Path))
                .arg(positional("interface", Interface))
                .arg(positional("property", Property))
                .arg(positional("signature", None))
                .arg(positional_vec("value", None)),
            subcommand("monitor")
                .arg(positional_vec("services", Service))
                .arg(opt("interface"))
                .arg(opt("member"))
                .arg(opt("path"))
                .arg(opt("sender"))
                .arg(Arg::new("match").long("match").value_name("MATCH").action(ArgAction::Set))
                .arg(flag("signals"))
                .arg(Arg::new("limit_messages").long("limit-messages").value_name("N").action(ArgAction::Set))
                .arg(Arg::new("timeout").long("timeout").value_name("DUR").action(ArgAction::Set)),
            Command::new("completion")
                .about("Generate shell completion script")
                .arg(
                    Arg::new("shell")
                        .value_name("SHELL")
                        .required(true)
                        .value_parser(["bash", "elvish", "fish", "powershell", "zsh"]),
                ),
        ])
}

/// The "kind" of bus value a positional holds. `None` ⇒ no dynamic completion
/// (e.g. method args, property values — out of scope for v1).
#[derive(Clone, Copy)]
enum Kind {
    Service,
    Path,
    Interface,
    Method,
    Signature,
    Property,
}

// Short lowercase aliases read better at the `positional(...)` call sites than
// `Some(Kind::Service)`; they're local ergonomics, not exported constants.
#[allow(non_upper_case_globals)]
const Service: Option<Kind> = Some(Kind::Service);
#[allow(non_upper_case_globals)]
const Path: Option<Kind> = Some(Kind::Path);
#[allow(non_upper_case_globals)]
const Interface: Option<Kind> = Some(Kind::Interface);
#[allow(non_upper_case_globals)]
const Method: Option<Kind> = Some(Kind::Method);
#[allow(non_upper_case_globals)]
const Signature: Option<Kind> = Some(Kind::Signature);
#[allow(non_upper_case_globals)]
const Property: Option<Kind> = Some(Kind::Property);

fn subcommand(name: &'static str) -> Command {
    Command::new(name).about(match name {
        "list" => "List service names on the bus",
        "tree" => "Show the object path tree of a service",
        "introspect" => "Show interfaces/methods/signals/properties of an object",
        "call" => "Call a method",
        "get" => "Get properties",
        "set" => "Set a property",
        "monitor" => "Monitor bus messages",
        _ => "",
    })
}

fn flag(name: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::SetTrue)
}

fn opt(name: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::Set)
}

/// A required positional with optional dynamic completion.
fn positional(name: &'static str, kind: Option<Kind>) -> Arg {
    let arg = Arg::new(name).required(true).action(ArgAction::Set);
    attach(arg, kind)
}

/// An optional (nullable) positional.
fn positional_opt(name: &'static str, kind: Option<Kind>) -> Arg {
    let arg = Arg::new(name).action(ArgAction::Set);
    attach(arg, kind)
}

/// A variadic positional (`Vec<String>`).
fn positional_vec(name: &'static str, kind: Option<Kind>) -> Arg {
    let arg = Arg::new(name).num_args(0..).action(ArgAction::Append);
    attach(arg, kind)
}

/// Attach the `ArgValueCompleter` for `kind` (if any). The completer ignores
/// clap's value-hint default (clap would otherwise try filesystem completion for
/// an `Other`-hinted arg) by setting `ValueHint::Other` and supplying its own
/// candidates.
fn attach(arg: Arg, kind: Option<Kind>) -> Arg {
    let arg = arg.value_hint(ValueHint::Other);
    match kind {
        Some(Kind::Service) => arg.add(ArgValueCompleter::new(complete_service)),
        Some(Kind::Path) => arg.add(ArgValueCompleter::new(complete_path)),
        Some(Kind::Interface) => arg.add(ArgValueCompleter::new(complete_interface)),
        Some(Kind::Method) => arg.add(ArgValueCompleter::new(complete_method)),
        Some(Kind::Signature) => arg.add(ArgValueCompleter::new(complete_signature)),
        Some(Kind::Property) => arg.add(ArgValueCompleter::new(complete_property)),
        None => arg,
    }
}

/// Completer fn for the service positional.
fn complete_service(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Service, current)
}

/// Completer fn for the object-path positional.
fn complete_path(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Path, current)
}

/// Completer fn for the interface positional.
fn complete_interface(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Interface, current)
}

/// Completer fn for the method positional.
fn complete_method(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Method, current)
}

/// Completer fn for the signature positional of `call`. Returns the method's
/// input signature (a single candidate), filtered by the partial token.
fn complete_signature(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Signature, current)
}

/// Completer fn for the property-name positional(s) of `get`/`set`. Lists the
/// property names of the chosen (or all) interface(s) on the object.
fn complete_property(current: &OsStr) -> Vec<CompletionCandidate> {
    complete_positional(Kind::Property, current)
}

/// The per-positional dynamic completer. Reads the bus flags + filled
/// positionals from `std::env::args_os()` (the same vector clap parses),
/// connects to the resulting bus, and dispatches to the matching introspection
/// helper. `current` is the partial token the user is typing.
fn complete_positional(kind: Kind, current: &OsStr) -> Vec<CompletionCandidate> {
    let parsed = match parse_args() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let Some(sub) = parsed.subcommand else {
        return Vec::new();
    };
    let current = match current.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let conn = match connect(parsed.user, parsed.system, parsed.address.as_deref(), parsed.verbose) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let cands = positional_candidates(&conn, sub, &parsed.positionals, kind, current)
        .unwrap_or_default();
    cands.into_iter().map(CompletionCandidate::new).collect()
}

/// Decoded view of the raw argv relevant to completion: the bus flags, the
/// subcommand, and the already-filled positional values (the partial being typed
/// is excluded — it arrives separately as `current`).
struct ParsedArgs {
    user: bool,
    system: bool,
    address: Option<String>,
    verbose: bool,
    subcommand: Option<&'static str>,
    positionals: Vec<String>,
}

/// Walk `std::env::args_os()` skipping the binary name, separating global flags
/// from the subcommand + its positionals. Flags after the subcommand (e.g.
/// `monitor --interface X`) are skipped so they don't masquerade as positionals.
/// Returns `None` only if argv can't be read at all.
fn parse_args() -> Option<ParsedArgs> {
    let mut user = false;
    let mut system = false;
    let mut address: Option<String> = None;
    let mut verbose = false;
    let mut subcommand: Option<&'static str> = None;
    let mut positionals: Vec<String> = Vec::new();

    let mut iter = std::env::args_os().skip(1);
    while let Some(raw) = iter.next() {
        let token = match raw.to_str() {
            Some(s) => s,
            None => continue,
        };
        if subcommand.is_none() {
            // Top-level: only globals + the subcommand name are expected here.
            match token {
                "--user" => user = true,
                "--system" => system = true,
                "--verbose" => verbose = true,
                "--address" => address = iter.next().and_then(|v| v.into_string().ok()),
                "--" => {}
                t if t.starts_with("--address=") => {
                    address = Some(t["--address=".len()..].to_string());
                }
                t if SERVICE_SUBS.contains(&t) || t == "list" || t == "completion" => {
                    // Record the subcommand name as a `&'static str`. The match
                    // arms below pin each branch to a literal, so the returned
                    // lifetime is `'static`.
                    subcommand = Some(match t {
                        "list" => "list",
                        "completion" => "completion",
                        "call" => "call",
                        "get" => "get",
                        "set" => "set",
                        "introspect" => "introspect",
                        "tree" => "tree",
                        "monitor" => "monitor",
                        _ => "call",
                    });
                }
                _ => {}
            }
        } else {
            // Inside a subcommand: skip flags (and their values for known
            // value-taking options) so only positionals are collected.
            if token == "--" {
                continue;
            }
            if let Some(rest) = token.strip_prefix("--") {
                // Split `--flag=value` into `(flag, Some(value))`; a bare `--flag`
                // is `(flag, None)` and may consume the *next* token as its value.
                let (flag, inline_value) = rest.split_once('=').map(|(f, v)| (f, Some(v))).unwrap_or((rest, None));
                let consume_next = inline_value.is_none() && takes_value(flag);
                match flag {
                    "address" => {
                        address = inline_value
                            .map(str::to_string)
                            .or_else(|| iter.next().and_then(|v| v.into_string().ok()));
                    }
                    _ if consume_next => {
                        let _ = iter.next();
                    }
                    _ => {}
                }
                continue;
            }
            if token.starts_with('-') && token.len() > 1 {
                continue;
            }
            positionals.push(token.to_string());
        }
    }

    // The last collected positional is the partial currently being typed; the
    // completer receives it separately as `current`, so drop it here. For a
    // required-but-empty position (user just typed the subcommand) the shell
    // appends an empty word, which lands here and is dropped correctly.
    if !positionals.is_empty() {
        positionals.pop();
    }

    Some(ParsedArgs { user, system, address, verbose, subcommand, positionals })
}

/// Whether a long flag (without the `--`) consumes a separate value token.
fn takes_value(name: &str) -> bool {
    matches!(
        name,
        "address" | "interface" | "member" | "path" | "sender" | "match" | "limit-messages"
            | "timeout"
    )
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
        ("call" | "get" | "set" | "introspect", Kind::Path) => {
            child_paths(conn, nth(0), partial)
        }
        ("call" | "get" | "set" | "introspect", Kind::Interface) => {
            interface_names(conn, nth(0), nth(1), partial)
        }
        ("call", Kind::Method) => method_names(conn, nth(0), nth(1), nth(2), partial),
        ("call", Kind::Signature) => method_input_signature_candidates(
            conn, nth(0), nth(1), nth(2), nth(3), partial,
        ),
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
/// single-candidate completion list filtered by the partial token. For a no-arg
/// method the signature is `""`, which is returned as-is so the user can accept
/// it.
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
    let proxy = zbus::blocking::Proxy::new(conn, service, path, INTROSPECTABLE)?;
    Ok(proxy.introspect()?)
}

/// Parse `<node name="..."/>` child entries from introspection XML.
fn parse_node_names(xml: &str) -> Vec<String> {
    let doc = match parse_doc(xml) {
        Some(d) => d,
        None => return Vec::new(),
    };
    doc.descendants()
        .filter(|n| n.has_tag_name("node"))
        .filter_map(|n| n.attribute("name").map(|s| s.to_string()))
        .collect()
}

/// Parse the interface names that are *direct children of the root `<node>`*
/// (the object's own interfaces, not those of registered sub-objects).
fn parse_root_interface_names(xml: &str) -> Vec<String> {
    let doc = match parse_doc(xml) {
        Some(d) => d,
        None => return Vec::new(),
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("interface"))
        .filter_map(|n| n.attribute("name").map(|s| s.to_string()))
        .collect()
}

/// Parse the method names of a given interface (a direct child of the root).
fn parse_interface_methods(xml: &str, interface: &str) -> Vec<String> {
    let doc = match parse_doc(xml) {
        Some(d) => d,
        None => return Vec::new(),
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("interface") && n.attribute("name") == Some(interface))
        .flat_map(|iface| {
            iface
                .children()
                .filter(|n| n.has_tag_name("method"))
                .filter_map(|n| n.attribute("name").map(|s| s.to_string()))
        })
        .collect()
}

/// Parse the property names of a given interface (a direct child of the root).
/// If `interface` is empty, collects properties from *all* of the root's own
/// interfaces — useful for `get` when the interface positional is omitted.
fn parse_interface_properties(xml: &str, interface: &str) -> Vec<String> {
    let doc = match parse_doc(xml) {
        Some(d) => d,
        None => return Vec::new(),
    };
    doc.root_element()
        .children()
        .filter(|n| {
            n.has_tag_name("interface")
                && (interface.is_empty() || n.attribute("name") == Some(interface))
        })
        .flat_map(|iface| {
            iface
                .children()
                .filter(|n| n.has_tag_name("property"))
                .filter_map(|n| n.attribute("name").map(|s| s.to_string()))
        })
        .collect()
}

/// Concatenate the `type` attribute of every `<arg direction="in">` child of the
/// named `<method>` in the named `<interface>`. Returns `None` if the document
/// can't be parsed or the method is absent; returns `Some("")` for a method that
/// takes no input args.
fn parse_method_input_signature(xml: &str, interface: &str, method: &str) -> Option<String> {
    let doc = parse_doc(xml)?;
    let iface = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("interface") && n.attribute("name") == Some(interface))?;
    let m = iface
        .children()
        .find(|n| n.has_tag_name("method") && n.attribute("name") == Some(method))?;
    let sig: String = m
        .children()
        .filter(|n| n.has_tag_name("arg") && n.attribute("direction") == Some("in"))
        .filter_map(|n| n.attribute("type").map(|s| s.to_string()))
        .collect();
    Some(sig)
}

/// Parse introspection XML with DTD support (zbus XML ships a `<!DOCTYPE>`).
/// Returns `None` on parse failure so callers degrade to empty candidates.
fn parse_doc(xml: &str) -> Option<roxmltree::Document<'_>> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    roxmltree::Document::parse_with_options(xml, opts).ok()
}
