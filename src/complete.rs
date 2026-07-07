//! `busx __complete` — dynamic shell-completion candidate generator (spec §12).
//!
//! Hidden subcommand invoked by the script emitted from `busx completion`. It
//! live-introspects the bus (no caching) and prints one candidate per line for
//! the positional currently being typed. The contract is best-effort: on any
//! bus error it prints nothing and the shell falls back to no completion, and
//! the command itself never fails (returns `Ok(())`).

use crate::conn::connect;
use crate::error::Result;
use zbus::blocking::Connection;
use zbus::blocking::fdo::DBusProxy;

/// The interface whose `Introspect` method we call. Every object implements it.
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Entry point for `busx __complete ARGS...`.
///
/// `args[0]` is the subcommand (`call`, `get`, ...); the elements after it are
/// the already-filled positionals followed by the partial token being typed
/// (which may be empty). `user`/`system`/`address`/`verbose` select the bus.
pub fn run(args: &[String], user: bool, system: bool, address: Option<&str>, verbose: bool) -> Result<()> {
    // Connect best-effort: if the bus isn't reachable, completion simply yields
    // nothing rather than surfacing an error to the shell.
    let conn = match connect(user, system, address, verbose) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // `split_first` guarantees `sub` exists; the rest splits into the filled
    // positionals (`rest` minus its last element) and the partial (`rest`'s last
    // element, or "" when only the subcommand was supplied).
    let (sub, rest) = match args.split_first() {
        Some((s, rest)) => (s.as_str(), rest),
        None => return Ok(()),
    };
    let filled: &[String] = &rest[..rest.len().saturating_sub(1)];
    let partial = rest.last().map(|s| s.as_str()).unwrap_or("");

    let cands = candidates(&conn, sub, filled, partial).unwrap_or_default();
    for c in cands {
        println!("{c}");
    }
    Ok(())
}

/// Decide which positional is being completed (`filled.len()`) and dispatch to
/// the matching introspection. Returns `Ok(vec![])` for unknown shapes so the
/// generator stays best-effort even here.
fn candidates(conn: &Connection, sub: &str, filled: &[String], partial: &str) -> Result<Vec<String>> {
    // Position index being completed == number of already-filled positionals.
    let pos = filled.len();
    match (sub, pos) {
        // 1st positional of the bus-walking subcommands → well-known services.
        ("call" | "get" | "set" | "introspect" | "tree" | "monitor", 0) => {
            service_names(conn, partial)
        }
        // 2nd positional → object path. Walk one level under `/` so the shell
        // re-completes the next segment; full path discovery is `tree`'s job.
        ("call" | "get" | "set" | "introspect", 1) => child_paths(conn, &filled[0], partial),
        // 3rd positional → interface name from the object's introspection.
        ("call" | "get" | "set" | "introspect", 2) => {
            interface_names(conn, &filled[0], &filled[1], partial)
        }
        // 4th positional of `call` → method name from the chosen interface.
        ("call", 3) => method_names(conn, &filled[0], &filled[1], &filled[2], partial),
        _ => Ok(vec![]),
    }
}

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
/// level is expanded — the shell re-invokes `__complete` for the next segment.
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
fn interface_names(conn: &Connection, service: &str, object: &str, partial: &str) -> Result<Vec<String>> {
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

/// Parse introspection XML with DTD support (zbus XML ships a `<!DOCTYPE>`).
/// Returns `None` on parse failure so callers degrade to empty candidates.
fn parse_doc(xml: &str) -> Option<roxmltree::Document<'_>> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    roxmltree::Document::parse_with_options(xml, opts).ok()
}

/// Emit a completion script for `shell` that calls back into `busx __complete`.
///
/// The script defines a completion function which forwards the words being
/// typed (minus `busx` itself) to `busx __complete` and feeds its newline
/// output to `compgen`/`compadd`. Static subcommand completion isn't useful
/// here — the value of busx completion is live bus introspection — so the
/// script is hand-rolled rather than generated by `clap_complete`.
pub fn emit_script(shell: clap_complete::Shell) {
    let script = match shell {
        clap_complete::Shell::Bash => bash_script(),
        clap_complete::Shell::Zsh => zsh_script(),
        // Other shells aren't exercised; emit nothing rather than a wrong guess.
        _ => return,
    };
    print!("{script}");
}

/// Bash completion script. `COMP_WORDS[@]:1` drops `busx` (word 0); the rest is
/// passed verbatim so `__complete` sees `[SUB, ...filled, partial]`.
fn bash_script() -> String {
    r#"# busx bash completion (dynamic — calls back into `busx __complete`).
_busx() {
    local IFS=$'\n'
    local cands
    cands=$(busx __complete "${COMP_WORDS[@]:1}" 2>/dev/null) || return
    COMPREPLY=($(compgen -W "$cands" -- "${COMP_WORDS[COMP_CWORD]}"))
    return 0
}
complete -o nospace -F _busx busx
"#.to_string()
}

/// Zsh completion script. `words[2,-1]` skips `busx` (word 1 in zsh indexing).
fn zsh_script() -> String {
    r#"#compdef busx
# busx zsh completion (dynamic — calls back into `busx __complete`).
_busx() {
    local -a cands
    cands=("${(@f)$(busx __complete "${words[2,-1]}" 2>/dev/null)}") || return
    compadd -- "${cands[@]}"
}
_busx "$@"
"#.to_string()
}
