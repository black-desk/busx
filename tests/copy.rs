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

// --- method call: complex (array) arg → full conversion per tool ---

#[test]
fn call_array_arg_each_tool() {
    // busctl lays out `as` as `count elem…` → ["2", "a", "b"].
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Take".into(),
        signature: "as".into(),
        args: vec!["2".into(), "a".into(), "b".into()],
    };
    // busctl: 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Take as 2 a b"
    );
    // dbus-send: array:string:a,b (man dbus-send BNF `array:<type>:<v>,<v>`).
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Take array:string:a,b"
    );
    // qdbus: drop the count, pass each element positionally.
    assert_eq!(
        cmd(&op, Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Take a b"
    );
    // gdbus: GVariant array literal ["a","b"] (GVariant text strings are
    // double-quoted per the GVariant text format spec).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Take [\"a\",\"b\"]"
    );
}

// --- method call: dict arg (a{ss}) → per-tool dict forms ---

#[test]
fn call_dict_arg_each_tool() {
    // busctl lays out `a{ss}` as `count key val …` → ["1", "k", "v"].
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Map".into(),
        signature: "a{ss}".into(),
        args: vec!["1".into(), "k".into(), "v".into()],
    };
    // busctl: 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Map a{ss} 1 k v"
    );
    // dbus-send: dict:string:string:k,v (man dbus-send BNF
    // `dict:<keytype>:<valtype>:<key>,<value>`).
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Map \
         dict:string:string:k,v"
    );
    // qdbus: no positional dict syntax → can't express the op (None).
    assert!(generate(&op, Tool::Qdbus).is_none());
    // gdbus: GVariant dict literal {"k":"v"}.
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Map {\"k\":\"v\"}"
    );
}

// --- method call: variant arg (v) → per-tool variant forms ---

#[test]
fn call_variant_arg_each_tool() {
    // busctl lays out `v` as `inner-signature value` → ["s", "hi"].
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Echo".into(),
        signature: "v".into(),
        args: vec!["s".into(), "hi".into()],
    };
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Echo v s hi"
    );
    // dbus-send: variant:string:hi (man dbus-send BNF `variant:<type>:<value>`).
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Echo variant:string:hi"
    );
    // qdbus: variant:hi.
    assert_eq!(
        cmd(&op, Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Echo variant:hi"
    );
    // gdbus: GVariant variant literal <"hi">.
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Echo <\"hi\">"
    );
}

// --- method call: struct arg ((ii)) → gdbus full; dbus-send/qdbus honest note ---

#[test]
fn call_struct_arg_each_tool() {
    // busctl lays out `(ii)` flat → ["1", "2"].
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Point".into(),
        signature: "(ii)".into(),
        args: vec!["1".into(), "2".into()],
    };
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Point (ii) 1 2"
    );
    // dbus-send: structs are not in the dbus-send BNF → can't express (None).
    assert!(generate(&op, Tool::DbusSend).is_none());
    // qdbus: no positional struct syntax → can't express (None).
    assert!(generate(&op, Tool::Qdbus).is_none());
    // gdbus: GVariant struct literal (1,2).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Point (1,2)"
    );
}

// --- method call: nested a{sv} → dbus-send/qdbus note; gdbus full nesting ---

#[test]
fn call_nested_asv_each_tool() {
    // busctl lays out `a{sv}` as `count key <inner-sig> <inner-value> …`.
    // One entry: key "k", variant of signature "s" value "v".
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Hints".into(),
        signature: "a{sv}".into(),
        args: vec!["1".into(), "k".into(), "s".into(), "v".into()],
    };
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Hints a{sv} 1 k s v"
    );
    // dbus-send forbids nested containers (a{sv}'s value is a variant) → None.
    assert!(generate(&op, Tool::DbusSend).is_none());
    // qdbus → None.
    assert!(generate(&op, Tool::Qdbus).is_none());
    // gdbus: full nesting — {"k":<"v">} (dict value is a GVariant variant).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Hints {\"k\":<\"v\">}"
    );
}

// --- method call: nested array aas → gdbus nested; dbus-send/qdbus note ---

#[test]
fn call_nested_aas_each_tool() {
    // `aas`: one outer element which is a 2-element string array.
    // busctl: `1 2 x y` (outer count 1, inner count 2, x, y).
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Grid".into(),
        signature: "aas".into(),
        args: vec!["1".into(), "2".into(), "x".into(), "y".into()],
    };
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Grid aas 1 2 x y"
    );
    // dbus-send forbids nested containers → None.
    assert!(generate(&op, Tool::DbusSend).is_none());
    // qdbus → None.
    assert!(generate(&op, Tool::Qdbus).is_none());
    // gdbus: [["x","y"]] (one outer array element, itself a 2-string array).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Grid [[\"x\",\"y\"]]"
    );
}

// --- method call: empty containers → dbus-send note; gdbus empty literal ---

#[test]
fn call_empty_containers_each_tool() {
    // dbus-send forbids empty containers (`man dbus-send`); gdbus/qdbus can.
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Take".into(),
        signature: "as".into(),
        args: vec!["0".into()],
    };
    // busctl: 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Take as 0"
    );
    // dbus-send: empty array is forbidden → can't express (None).
    assert!(generate(&op, Tool::DbusSend).is_none());
    // qdbus: empty array expands to zero positional args (no note).
    assert_eq!(cmd(&op, Tool::Qdbus), "qdbus org.busx.Test /o org.busx.Test.Take");
    // gdbus: empty array literal [].
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Take []"
    );
}

// --- method call: partial args (array with only the count token) → no panic ---

#[test]
fn call_partial_array_args_render_placeholder() {
    // `as` with only the count token "2" — both elements missing.
    let op = CopyOp::Call {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        method: "Take".into(),
        signature: "as".into(),
        args: vec!["2".into()],
    };
    // busctl: 1:1 (the missing tokens are simply absent).
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl call org.busx.Test /o org.busx.Test Take as 2"
    );
    // dbus-send: count 2, two missing elements → `?` placeholders each.
    assert_eq!(
        cmd(&op, Tool::DbusSend),
        "dbus-send --print-reply --dest=org.busx.Test /o org.busx.Test.Take array:string:?,?"
    );
    // qdbus: count 2, two missing elements → two quoted `?` positional args
    // (`?` is a shell glob, so each element is quoted).
    assert_eq!(
        cmd(&op, Tool::Qdbus),
        "qdbus org.busx.Test /o org.busx.Test.Take \"?\" \"?\""
    );
    // gdbus: count 2, two missing elements → ["?","?"].
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.busx.Test.Take [\"?\",\"?\"]"
    );
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
    // value. Properties.Set's last arg is a variant (`ssv`), so the value is a
    // GVariant variant literal `<"hi">` (per `man gdbus`: "serialized GVariant").
    assert_eq!(
        cmd(&set_op(), Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Set \"org.busx.Test\" \"Name\" <\"hi\">"
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
    // gdbus emits the uint32 value as a bare number, wrapped in a GVariant
    // variant `<7>` (Properties.Set's last arg is `v`).
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Set \"org.busx.Test\" \"Count\" <7>"
    );
    // busctl is 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl set-property org.busx.Test /o org.busx.Test Count u 7"
    );
}

// --- property set (complex): gdbus wraps in variant; dbus-send/qdbus honest ---

#[test]
fn set_array_property_each_tool() {
    // Set an `as` property to ["a","b"] — busctl value tokens: 2 a b.
    let op = CopyOp::Set {
        service: "org.busx.Test".into(),
        object: "/o".into(),
        iface: "org.busx.Test".into(),
        property: "Tags".into(),
        signature: "as".into(),
        value: vec!["2".into(), "a".into(), "b".into()],
    };
    // busctl: 1:1.
    assert_eq!(
        cmd(&op, Tool::Busctl),
        "busctl set-property org.busx.Test /o org.busx.Test Tags as 2 a b"
    );
    // dbus-send Properties.Set: the property value is a variant; dbus-send's
    // variant inner type must be basic, and `as` is not → can't express (None).
    assert!(generate(&op, Tool::DbusSend).is_none());
    // qdbus Properties.Set: array not expressible positionally → None.
    assert!(generate(&op, Tool::Qdbus).is_none());
    // gdbus Properties.Set: array value wrapped in a GVariant variant `<[...]>`.
    assert_eq!(
        cmd(&op, Tool::Gdbus),
        "gdbus call --session --dest org.busx.Test --object-path /o \
         --method org.freedesktop.DBus.Properties.Set \
         \"org.busx.Test\" \"Tags\" <[\"a\",\"b\"]>"
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
