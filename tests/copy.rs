// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Direct tests for `busx::tui::copy::generate` (spec §10 copy-as generation).
//! Pin the exact command each tool produces for representative operations.

use busx::tui::copy::{generate, CopyOp, Tool};

/// Helper: render `op` for `tool`, unwrapping the `Some(command)`.
fn cmd(op: &CopyOp, tool: Tool) -> String {
    generate(op, tool).expect("expected Some(command)")
}

// --- method call: single basic arg (Add(n: u) → signature "u", args ["42"]) ---

fn call_add_u() -> CopyOp {
    CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Add".into(),
        signature: "u".into(),
        args: vec!["42".into()],
    }
}

#[test]
fn call_basic_u_busctl() {
    assert_eq!(
        cmd(&call_add_u(), Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Add u 42"
    );
}

#[test]
fn call_basic_u_dbus_send() {
    assert_eq!(
        cmd(&call_add_u(), Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Add uint32:42"
    );
}

#[test]
fn call_basic_u_qdbus() {
    assert_eq!(
        cmd(&call_add_u(), Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Add 42"
    );
}

#[test]
fn call_basic_u_gdbus() {
    assert_eq!(
        cmd(&call_add_u(), Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o --method org.busx.Test.Add 42"
    );
}

// --- method call: two basic args (signature "su", args ["hi","7"]) ---

fn call_su() -> CopyOp {
    CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Echo".into(),
        signature: "su".into(),
        args: vec!["hi".into(), "7".into()],
    }
}

#[test]
fn call_two_args_busctl() {
    // busctl: interface and method are separate positional args (space-separated,
    // not dotted like dbus-send/qdbus/gdbus).
    assert_eq!(
        cmd(&call_su(), Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Echo su hi 7"
    );
}

#[test]
fn call_two_args_dbus_send() {
    assert_eq!(
        cmd(&call_su(), Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Echo string:hi uint32:7"
    );
}

#[test]
fn call_two_args_qdbus() {
    // qdbus infers types from introspection — literal values, space-joined.
    assert_eq!(
        cmd(&call_su(), Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Echo hi 7"
    );
}

#[test]
fn call_two_args_gdbus() {
    // gdbus: string `"hi"` (quoted), bare `7` for the uint32.
    assert_eq!(
        cmd(&call_su(), Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o --method org.busx.Test.Echo \"hi\" 7"
    );
}

// --- method call: zero args (signature "", no args) ---

#[test]
fn call_zero_args_busctl_omits_signature() {
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Ping".into(),
        signature: "".into(),
        args: vec![],
    };
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Ping"
    );
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Ping"
    );
    assert_eq!(cmd(&op, Tool::Qdbus), "qdbus org.busx.Test /o org.busx.Test.Ping");
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o --method org.busx.Test.Ping"
    );
}

// --- method call: complex (array) arg → best-effort + note ---

#[test]
fn call_array_arg_dbus_send_notes_cannot_nest() {
    // busctl lays out `as` as `count elem…` → ["2", "a", "b"].
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Take".into(),
        signature: "as".into(),
        args: vec!["2".into(), "a".into(), "b".into()],
    };
    // dbus-send can't express arrays like busx → best-effort + note.
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Take variant:2\n\
         # dbus-send cannot fully express signature \"as\""
    );
    // qdbus drops the busctl count prefix and joins the elements.
    assert_eq!(
        cmd(&op, Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Take a b"
    );
    // gdbus flags the complex type as best-effort.
    let g = cmd(&op, Tool::Gdbus);
    assert!(g.starts_with(
        "gdbus call --session --dest org.busx.Test --object-path /o --method org.busx.Test.Take"
    ));
    assert!(g.contains("# gdbus: complex-type args are best-effort GVariant text"));
}

// --- property get: all four ---

fn get_op() -> CopyOp {
    CopyOp::Get {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        property: "Name".into(),
    }
}

#[test]
fn get_busctl() {
    assert_eq!(
        cmd(&get_op(), Tool::Busctl),
        "busctl get-property org.busx.Test /o org.busx.Test Name"
    );
}

#[test]
fn get_dbus_send() {
    assert_eq!(
        cmd(&get_op(), Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o \
         org.freedesktop.DBus.Properties.Get string:org.busx.Test string:Name"
    );
}

#[test]
fn get_qdbus() {
    // PINNED: qdbus has no first-class property read syntax; we route through
    // the standard Properties.Get method with `--literal` for raw output.
    assert_eq!(
        cmd(&get_op(), Tool::Qdbus),
        "qdbus --literal org.busx.Test /o org.freedesktop.DBus.Properties.Get org.busx.Test Name"
    );
}

#[test]
fn get_gdbus() {
    // PINNED: gdbus Properties.Get takes two GVariant string args; gdbus
    // infers the 's' type from the known (ss) signature, so bare quoted
    // strings (not the '<...>' variant form) are correct.
    assert_eq!(
        cmd(&get_op(), Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Get \"org.busx.Test\" \"Name\""
    );
}

// --- property set (basic): all four ---

fn set_op() -> CopyOp {
    CopyOp::Set {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        property: "Name".into(),
        signature: "s".into(),
        value: vec!["hi".into()],
    }
}

#[test]
fn set_busctl() {
    assert_eq!(
        cmd(&set_op(), Tool::Busctl),
        "busctl set-property org.busx.Test /o org.busx.Test Name s hi"
    );
}

#[test]
fn set_dbus_send() {
    assert_eq!(
        cmd(&set_op(), Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o \
         org.freedesktop.DBus.Properties.Set string:org.busx.Test string:Name variant:string:hi"
    );
}

#[test]
fn set_qdbus() {
    // PINNED: qdbus Properties.Set: interface, property, `variant:<value>`
    // (qdbus wraps it in a QDBusVariant).
    assert_eq!(
        cmd(&set_op(), Tool::Qdbus),
        "qdbus org.busx.Test /o org.freedesktop.DBus.Properties.Set org.busx.Test Name variant:hi"
    );
}

#[test]
fn set_gdbus() {
    // PINNED: gdbus Properties.Set takes interface, property, and a GVariant
    // value (the property value). For a string property the value is `"hi"`.
    assert_eq!(
        cmd(&set_op(), Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Set \"org.busx.Test\" \"Name\" \"hi\""
    );
}

// --- property set (non-string basic): gdbus emits bare numbers ---

#[test]
fn set_uint_gdbus_bare_number() {
    let op = CopyOp::Set {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        property: "Count".into(),
        signature: "u".into(),
        value: vec!["7".into()],
    };
    // gdbus emits the uint32 value as a bare number (best-effort: no `@u`
    // annotation — gdbus infers the property type from introspection).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Set \"org.busx.Test\" \"Count\" 7"
    );
    // busctl is 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl set-property org.busx.Test /o org.busx.Test Count u 7"
    );
}

// --- listen: match rule across all four ---

fn listen_op() -> CopyOp {
    CopyOp::Listen { rule: "type='signal'".into() }
}

#[test]
fn listen_dbus_send_is_dbus_monitor() {
    assert_eq!(cmd(&listen_op(), Tool::DbusSend), "dbus-monitor \"type='signal'\"");
}

#[test]
fn listen_busctl_is_busctl_monitor() {
    assert_eq!(cmd(&listen_op(), Tool::Busctl), "busctl monitor \"type='signal'\"");
}

#[test]
fn listen_qdbus_is_none() {
    // qdbus has no monitor facility.
    assert_eq!(generate(&listen_op(), Tool::Qdbus), None);
}

#[test]
fn listen_gdbus_is_bare_command_plus_note() {
    // PINNED: gdbus monitor is unfiltered (it ignores match rules), so emit
    // the bare command + a note rather than dropping the user.
    assert_eq!(
        generate(&listen_op(), Tool::Gdbus),
        Some("gdbus monitor --session\n# gdbus monitor is unfiltered — it ignores the rule".into())
    );
}

// --- Tool metadata ---

#[test]
fn tool_all_is_four_tools_in_order() {
    assert_eq!(
        Tool::ALL,
        [Tool::DbusSend, Tool::Busctl, Tool::Qdbus, Tool::Gdbus]
    );
}

#[test]
fn tool_name_is_the_command() {
    assert_eq!(Tool::DbusSend.name(), "dbus-send");
    assert_eq!(Tool::Busctl.name(), "busctl");
    assert_eq!(Tool::Qdbus.name(), "qdbus");
    assert_eq!(Tool::Gdbus.name(), "gdbus");
}
