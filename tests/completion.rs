// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end tests for `clap_complete::dynamic` completion.
//!
//! These drive the *real* completion entry point the way the shell does:
//! invoking the binary with `COMPLETE=<shell>` set plus the `-- <words>` argv
//! that `clap_complete`'s registration script emits. This is the layer the old
//! `__complete` tests skipped, which is exactly what let bugs #1/#2/#3 ship.

mod common;
use assert_cmd::Command;

/// Drive `clap_complete`'s bash completion for the given words, returning the
/// newline-joined candidate output. `index` is the 1-based COMP_CWORD (position
/// within `words`, where `busx` is word 0).
fn complete_bash(words: &[&str], index: usize) -> String {
    let mut cmd = Command::cargo_bin("busx").unwrap();
    cmd.env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", index.to_string())
        .env("_CLAP_IFS", "\n")
        .arg("--");
    for w in words {
        cmd.arg(w);
    }
    let out = cmd.ok().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// === Bug #1: subcommand position completes =================================

/// `busx <TAB>` lists the subcommands. Previously the hand-rolled script only
/// ever called `__complete`, which returned nothing for the subcommand slot.
#[test]
fn complete_subcommand_position_lists_subcommands() {
    let out = complete_bash(&["busx", ""], 1);
    for sub in [
        "call",
        "get",
        "set",
        "list",
        "introspect",
        "monitor",
        // "completion" is `#[command(hide = true)]` so it's deliberately not
        // offered — now that completion uses the real Cli (not the old mirror,
        // which forgot to replicate `hide`).
    ] {
        assert!(
            out.lines().any(|l| l == sub),
            "subcommand `{sub}` missing from:\n{out}"
        );
    }
}

/// Prefix matching at the subcommand position narrows candidates (`c` → call /
/// completion), so the shell isn't flooded with every command.
#[test]
fn complete_subcommand_position_filters_by_prefix() {
    let out = complete_bash(&["busx", "c"], 1);
    let cands: Vec<&str> = out.lines().filter(|l| !l.starts_with('-')).collect();
    assert!(cands.contains(&"call"), "call missing: {out}");
    // "completion" is hidden, so `c` narrows to just "call".
    assert!(
        !cands.contains(&"completion"),
        "hidden `completion` subcommand leaked: {out}"
    );
    assert!(!cands.contains(&"list"), "non-matching leaked: {out}");
}

/// Completing right after `busx` also offers the global flags (bug #1's "flags
/// should complete" half). `--verbose` is now the repeatable short `-v`.
#[test]
fn complete_after_busx_offers_global_flags() {
    let out = complete_bash(&["busx", "-"], 1);
    for flag in ["--user", "--system", "--address", "-v"] {
        assert!(
            out.lines().any(|l| l == flag),
            "global flag `{flag}` missing from:\n{out}"
        );
    }
}

// === Bug #2/#3: global-flag forwarding to the positional completer ==========

/// `busx --address <fixture> call <TAB>` connects to the fixture bus and returns
/// its `org.busx.Test`. Previously the script passed `--address` to `__complete`
/// as if it were the subcommand, and even when fixed the generator ignored the
/// flag and always hit the session bus.
#[test]
fn complete_service_uses_address_bus() {
    let addr = common::bus().address.clone();
    // words: busx --address <ADDR> call "" ; cursor on the empty service slot.
    let out = complete_bash(&["busx", "--address", &addr, "call", ""], 4);
    assert!(
        out.lines().any(|l| l == "org.busx.Test"),
        "fixture service missing (completer didn't use --address):\n{out}"
    );
}

/// Proves the completer connects to the *requested* bus, not the session bus: an
/// unreachable `--address` yields no service candidates at all.
#[test]
fn complete_service_dead_address_yields_no_services() {
    let out = complete_bash(
        &[
            "busx",
            "--address",
            "unix:path=/nonexistent/cmp.sock",
            "call",
            "",
        ],
        4,
    );
    assert!(
        !out.lines().any(|l| l == "org.freedesktop.DBus"),
        "session-bus name leaked into a dead-address completion:\n{out}"
    );
}

/// `--system` is parsed by clap as a flag (not mis-parsed as the subcommand —
/// bug #2's system-bus variant). The proof: `call` is consumed as the
/// subcommand, so the engine completes the service positional rather than
/// treating `--system` as the command name. The system bus contents are
/// environment-dependent, so we don't pin a specific service; we only assert
/// the structural outcome (some well-known service name is offered, or at least
/// `call` was recognized — never re-listed as a subcommand candidate).
#[test]
fn complete_service_system_flag_runs() {
    let out = complete_bash(&["busx", "--system", "call", ""], 3);
    // `call` must have been consumed as the subcommand: completing the service
    // slot must NOT echo `call` back as a candidate (which would happen only if
    // `--system` were mistaken for the subcommand and `call` left as a fresh
    // subcommand-position token).
    assert!(
        !out.lines().any(|l| l == "call"),
        "`call` re-listed as a candidate — `--system` was mis-parsed:\n{out}"
    );
}

// === Positional live introspection (service/path/interface/method) =========

/// Path position: completing the object path of `introspect` introspects `/` and
/// offers the fixture's `/org` child.
#[test]
fn complete_path_position_lists_child_paths() {
    let addr = common::bus().address.clone();
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "",
        ],
        5,
    );
    assert!(
        out.lines().any(|l| l.starts_with("/org")),
        "child path candidate missing:\n{out}"
    );
}

/// Interface position: completing the interface of `introspect` lists the
/// object's interfaces, including the fixture's own and the standard ones.
#[test]
fn complete_interface_position_lists_interfaces() {
    let addr = common::bus().address.clone();
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
            "",
        ],
        6,
    );
    assert!(
        out.lines().any(|l| l == "org.busx.Test"),
        "fixture interface missing:\n{out}"
    );
    assert!(
        out.lines().any(|l| l == "org.freedesktop.DBus.Properties"),
        "standard interface missing: {out}"
    );
}

/// Method position: completing the method of `call` lists the chosen
/// interface's methods, including the fixture's `BumpVolume`.
#[test]
fn complete_method_position_lists_methods() {
    let addr = common::bus().address.clone();
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "",
        ],
        7,
    );
    assert!(
        out.lines().any(|l| l == "BumpVolume"),
        "method candidate BumpVolume missing:\n{out}"
    );
}

// === Signature completion (call's signature positional) =====================

/// Signature position: completing the signature of `Join` returns its input
/// signature `as`.
#[test]
fn complete_signature_position_returns_join_input_sig() {
    let addr = common::bus().address.clone();
    // words: busx(0) --address(1) <addr>(2) call(3) svc(4) obj(5) iface(6)
    //        Join(7) ""(8) ; cursor on the empty signature slot.
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "Join",
            "",
        ],
        8,
    );
    assert!(
        out.lines().any(|l| l == "as"),
        "signature candidate `as` missing:\n{out}"
    );
}

/// Signature position: completing the signature of `TakeHints` returns its input
/// signature `a{sv}`.
#[test]
fn complete_signature_position_returns_takehints_input_sig() {
    let addr = common::bus().address.clone();
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "TakeHints",
            "",
        ],
        8,
    );
    assert!(
        out.lines().any(|l| l == "a{sv}"),
        "signature candidate `a{{sv}}` missing:\n{out}"
    );
}

/// Signature position: a no-arg method (`BumpVolume`) yields the empty
/// signature. No real signature (e.g. `as`, `a{sv}`) should be offered. The
/// empty candidate itself is `""`; clap_complete may also surface the global
/// flags at an empty position, which is harmless structural behavior.
#[test]
fn complete_signature_position_no_arg_method_is_empty() {
    let addr = common::bus().address.clone();
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "BumpVolume",
            "",
        ],
        8,
    );
    // No real (type-code) signature candidate should be offered for a no-arg
    // method. Flags may legitimately appear; filter them out.
    let sigs: Vec<&str> = out
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('-'))
        .collect();
    assert!(
        sigs.is_empty(),
        "unexpected signature candidate(s) for no-arg method:\n{out}"
    );
}

// === Property-name completion (get/set property positional(s)) ==============

/// `get`'s property position: completing after `get SVC OBJ IFACE <TAB>` lists
/// the fixture's property names. `get` carries service(0)/object(1)/interface(2),
/// so the first prop sits at position 3.
#[test]
fn complete_get_property_position_lists_properties() {
    let addr = common::bus().address.clone();
    // words: busx(0) --address(1) <addr>(2) get(3) svc(4) obj(5) iface(6) ""(7)
    //        ; cursor on the empty property slot.
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "get",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "",
        ],
        7,
    );
    for prop in ["volume", "name", "counts", "hints"] {
        assert!(
            out.lines().any(|l| l == prop),
            "property `{prop}` missing:\n{out}"
        );
    }
}

/// `get`'s property position is variadic: a second `<TAB>` (after the first prop)
/// still completes property names, not nothing.
#[test]
fn complete_get_second_property_still_completes() {
    let addr = common::bus().address.clone();
    // words: busx(0) --address(1) <addr>(2) get(3) svc(4) obj(5) iface(6)
    //        volume(7) ""(8) ; cursor on the next variadic prop slot.
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "get",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "volume",
            "",
        ],
        8,
    );
    assert!(
        out.lines().any(|l| l == "name"),
        "second property `name` missing (variadic completion broke):\n{out}"
    );
}

/// `set`'s property position: completing after `set SVC OBJ IFACE <TAB>` lists
/// the fixture's property names. `set` carries service(0)/object(1)/interface(2),
/// so the property sits at position 3.
#[test]
fn complete_set_property_position_lists_properties() {
    let addr = common::bus().address.clone();
    // words: busx(0) --address(1) <addr>(2) set(3) svc(4) obj(5) iface(6) ""(7)
    //        ; cursor on the empty property slot.
    let out = complete_bash(
        &[
            "busx",
            "--address",
            &addr,
            "set",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "",
        ],
        7,
    );
    for prop in ["volume", "name", "counts", "hints"] {
        assert!(
            out.lines().any(|l| l == prop),
            "property `{prop}` missing:\n{out}"
        );
    }
}

// === Robustness ============================================================

/// Completion never fails the command even when the bus is unreachable: an
/// invalid `--address` yields exit 0 and (for the positional) no bus names.
#[test]
fn complete_silently_yields_nothing_on_bus_error() {
    let out = complete_bash(
        &[
            "busx",
            "--address",
            "unix:path=/nonexistent/busx.sock",
            "call",
            "",
        ],
        4,
    );
    // Global flags may still appear (they're valid at this position), but no
    // real service names should leak from the session bus.
    assert!(
        !out.lines().any(|l| l == "org.freedesktop.DBus"),
        "session-bus name leaked on a bus error: {out}"
    );
}

/// The `completion` subcommand still emits the registration script that
/// `clap_complete::dynamic` expects for bash (i.e. our `busx completion bash`
/// now mirrors `source <(COMPLETE=bash busx)`).
#[test]
fn completion_subcommand_emits_bash_registration() {
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["completion", "bash"])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("complete ") || stdout.contains("compdef") || stdout.contains("busx"),
        "registration script should reference the binary:\n{stdout}"
    );
}
