// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render a busx operation as another D-Bus tool's command line (spec §10).
//!
//! Pure: no IO, no clipboard — the copy-as popup (Phase 5 Task 2) calls
//! [`generate`]. busx's method/property-set args are **busctl-style**
//! (`crate::value::encode`): a separate signature string plus positional
//! value tokens (basic → one token; variant → inner-sig+value; array →
//! count+N; dict → count+N pairs; struct → flat). So [`Tool::Busctl`]
//! copy-as is **1:1** — the signature and tokens map directly onto
//! `busctl call`/`set-property`. The other tools convert: basic types are
//! exact; complex types (arrays/structs/variants/dicts) are best-effort
//! (dbus-send/qdbus/gdbus have limited or different nested syntax), with a
//! trailing `# …` note where a tool genuinely cannot express the signature.
//!
//! `--user`/`--system` bus flags are intentionally omitted (the user adds
//! the bus they want); for gdbus we emit the placeholder `--session`.

/// An operation we can render as another tool's command.
#[derive(Clone, Debug)]
pub enum CopyOp {
    /// Method call. `signature` is the method's IN-signature; `args` are
    /// busctl-style value tokens laid out per [`crate::value::encode`].
    Call {
        service: String,
        object: String,
        iface: String,
        method: String,
        signature: String,
        args: Vec<String>,
    },
    /// Property get.
    Get {
        service: String,
        object: String,
        iface: String,
        property: String,
    },
    /// Property set. `signature` + `value` are busctl-style (one value's
    /// worth of tokens).
    Set {
        service: String,
        object: String,
        iface: String,
        property: String,
        signature: String,
        value: Vec<String>,
    },
    /// Signal/property/method listen. `rule` is the D-Bus match-rule string.
    Listen { rule: String },
}

/// The other D-Bus tool to render as. Listed in [`Tool::ALL`] in popup order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    DbusSend,
    Busctl,
    Qdbus,
    Gdbus,
}

impl Tool {
    /// All tools, in popup-display order.
    pub const ALL: [Tool; 4] = [Tool::DbusSend, Tool::Busctl, Tool::Qdbus, Tool::Gdbus];

    /// The tool's command name (e.g. `"dbus-send"`).
    pub fn name(self) -> &'static str {
        match self {
            Self::DbusSend => "dbus-send",
            Self::Busctl => "busctl",
            Self::Qdbus => "qdbus",
            Self::Gdbus => "gdbus",
        }
    }
}

/// Render `op` as `tool`'s command line.
///
/// Returns `None` only where the tool genuinely cannot express the operation
/// (qdbus has no monitor). Best-effort outputs carry a trailing `# …` note
/// where a tool can't fully express a signature.
pub fn generate(op: &CopyOp, tool: Tool) -> Option<String> {
    match op {
        CopyOp::Call { service, object, iface, method, signature, args } => {
            Some(call(service, object, iface, method, signature, args, tool))
        }
        CopyOp::Get { service, object, iface, property } => {
            Some(get(service, object, iface, property, tool))
        }
        CopyOp::Set { service, object, iface, property, signature, value } => {
            Some(set(service, object, iface, property, signature, value, tool))
        }
        CopyOp::Listen { rule } => listen(rule, tool),
    }
}

// --- method call -----------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn call(
    service: &str,
    object: &str,
    iface: &str,
    method: &str,
    signature: &str,
    args: &[String],
    tool: Tool,
) -> String {
    match tool {
        // 1:1 — the args are already busctl tokens. Quote each and space-join.
        Tool::Busctl => {
            let mut parts: Vec<String> = vec![
                "busctl".into(),
                "call".into(),
                service.into(),
                object.into(),
                iface.into(),
                method.into(),
            ];
            if !signature.is_empty() {
                parts.push(signature.into());
            }
            for a in args {
                parts.push(quote(a));
            }
            parts.join(" ")
        }
        Tool::DbusSend => {
            let mut out = format!(
                "dbus-send --print-reply --dest={service} {object} {iface}.{method}"
            );
            // Pair each top-level signature type with the positional tokens.
            // For basic types emit `type:value`; for complex types emit a
            // best-effort `type:value` and flag that dbus-send can't nest.
            let (note, tokens) = split_for_dbus_send(signature, args);
            for (type_tag, value) in tokens {
                out.push(' ');
                out.push_str(type_tag);
                out.push(':');
                out.push_str(&value);
            }
            if let Some(sig) = note {
                out.push_str(&format!("\n# dbus-send cannot fully express signature \"{sig}\""));
            }
            out
        }
        Tool::Qdbus => {
            let mut out = format!("qdbus {service} {object} {iface}.{method}");
            // qdbus infers types from introspection — emit the literal token
            // values, dropping the busctl count prefix for arrays. Best-effort
            // for complex types.
            for lit in qdbus_call_args(signature, args) {
                out.push(' ');
                out.push_str(&quote(&lit));
            }
            out
        }
        Tool::Gdbus => {
            let mut out = format!(
                "gdbus call --session --dest {service} --object-path {object} --method {iface}.{method}"
            );
            for gv in gdbus_call_args(signature, args) {
                out.push(' ');
                out.push_str(&gv);
            }
            if gdbus_has_complex(signature) {
                out.push_str("\n# gdbus: complex-type args are best-effort GVariant text");
            }
            out
        }
    }
}

// --- property get ----------------------------------------------------------

fn get(service: &str, object: &str, iface: &str, property: &str, tool: Tool) -> String {
    match tool {
        Tool::Busctl => format!("busctl get-property {service} {object} {iface} {property}"),
        Tool::DbusSend => format!(
            "dbus-send --print-reply --dest={service} {object} \
             org.freedesktop.DBus.Properties.Get string:{iface} string:{property}"
        ),
        // qdbus reads a property via the standard Properties.Get method
        // (qdbus has no first-class property syntax on the command line);
        // `--literal` returns the raw value without qdbus's pretty-printer.
        Tool::Qdbus => format!(
            "qdbus --literal {service} {object} org.freedesktop.DBus.Properties.Get {iface} {property}"
        ),
        // gdbus Properties.Get takes two GVariant string args; gdbus infers
        // the `'s'` type from the known signature `(ss)`, so bare quoted
        // strings work.
        Tool::Gdbus => format!(
            "gdbus call --session --dest {service} --object-path {object} \
             --method org.freedesktop.DBus.Properties.Get \"{iface}\" \"{property}\""
        ),
    }
}

// --- property set ----------------------------------------------------------

fn set(
    service: &str,
    object: &str,
    iface: &str,
    property: &str,
    signature: &str,
    value: &[String],
    tool: Tool,
) -> String {
    match tool {
        // 1:1 — signature + value tokens map straight onto set-property.
        Tool::Busctl => {
            let mut parts: Vec<String> = vec![
                "busctl".into(),
                "set-property".into(),
                service.into(),
                object.into(),
                iface.into(),
                property.into(),
                signature.into(),
            ];
            for v in value {
                parts.push(quote(v));
            }
            parts.join(" ")
        }
        Tool::DbusSend => {
            // dbus-send Properties.Set takes interface, property, and a
            // variant value. Best-effort: emit the property's type tag for
            // basic types; for complex property types fall back to variant:
            // with a note.
            let type_tag = dbus_send_type_tag(signature).unwrap_or("variant");
            let val = value.first().map(String::as_str).unwrap_or("");
            let complex = dbus_send_type_tag(signature).is_none() && !signature.is_empty();
            let mut out = format!(
                "dbus-send --print-reply --dest={service} {object} \
                 org.freedesktop.DBus.Properties.Set string:{iface} string:{property} \
                 variant:{type_tag}:{val}"
            );
            if complex {
                out.push_str(&format!(
                    "\n# dbus-send cannot fully express property signature \"{signature}\""
                ));
            }
            out
        }
        // qdbus Properties.Set: interface, property, then a `variant:...`
        // value (qdbus wraps it in a QDBusVariant). Best-effort.
        Tool::Qdbus => {
            let val = value.first().map(String::as_str).unwrap_or("");
            format!(
                "qdbus {service} {object} org.freedesktop.DBus.Properties.Set \
                 {iface} {property} variant:{val}"
            )
        }
        // gdbus Properties.Set: interface, property, then a GVariant value.
        // The value arg is itself a variant, so wrap basic values in `<>`.
        Tool::Gdbus => {
            let gv = gdbus_single_value(signature, value.first().map(String::as_str).unwrap_or(""));
            let complex = gdbus_has_complex(signature);
            let mut out = format!(
                "gdbus call --session --dest {service} --object-path {object} \
                 --method org.freedesktop.DBus.Properties.Set \
                 \"{iface}\" \"{property}\" {gv}"
            );
            if complex {
                out.push_str(&format!(
                    "\n# gdbus: property signature \"{signature}\" is best-effort GVariant text"
                ));
            }
            out
        }
    }
}

// --- listen ----------------------------------------------------------------

fn listen(rule: &str, tool: Tool) -> Option<String> {
    match tool {
        Tool::DbusSend => Some(format!("dbus-monitor {}", quote(rule))),
        Tool::Busctl => Some(format!("busctl monitor {}", quote(rule))),
        // qdbus has no monitor facility at all.
        Tool::Qdbus => None,
        // gdbus monitor is unfiltered (it ignores match rules), so emit the
        // bare command plus a note rather than dropping the user.
        Tool::Gdbus => Some(
            "gdbus monitor --session\n# gdbus monitor is unfiltered — it ignores the rule".into(),
        ),
    }
}

// --- helpers ---------------------------------------------------------------

/// Shell-quote a token if it needs it (whitespace or special chars), wrapping
/// in `"..."` and escaping `\` and `"`. Tokens that need no quoting pass
/// through unchanged.
fn quote(s: &str) -> String {
    let needs = s.is_empty()
        || s.chars().any(|c| {
            c.is_whitespace() || matches!(c, '"' | '\\' | '\'' | '$' | '`' | '<' | '>' | '&' | ';' | '|' | '*' | '?' | '(' | ')')
        });
    if !needs {
        return s.into();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Split a D-Bus signature into its top-level complete types
/// (e.g. `"su"` → `["s","u"]`, `"as"` → `["as"]`, `"(ii)s"` → `["(ii)","s"]`).
///
/// A complete type is: one basic code, an array `a…` (one complete element
/// type), a dict `a{KV}` (two complete types), or a struct `(...)` (complete
/// types until `)`). Mirrors the walker in [`crate::value::encode`].
fn split_signature(sig: &str) -> Vec<String> {
    let chars: Vec<char> = sig.chars().collect();
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < chars.len() {
        let start = pos;
        if let Err(()) = skip_complete_type(&chars, &mut pos) {
            break;
        }
        out.push(chars[start..pos].iter().collect());
    }
    out
}

/// Advance `pos` past one complete type, or return `Err(())` on truncation.
fn skip_complete_type(chars: &[char], pos: &mut usize) -> Result<(), ()> {
    match chars.get(*pos).copied() {
        None => Err(()),
        // basic types + variant
        Some('y' | 'b' | 'n' | 'q' | 'i' | 'u' | 'x' | 't' | 'd' | 's' | 'o' | 'g' | 'v') => {
            *pos += 1;
            Ok(())
        }
        Some('a') => {
            *pos += 1;
            match chars.get(*pos).copied() {
                Some('{') => {
                    *pos += 1; // consume '{'
                    skip_complete_type(chars, pos)?; // key
                    skip_complete_type(chars, pos)?; // value
                    match chars.get(*pos).copied() {
                        Some('}') => {
                            *pos += 1;
                            Ok(())
                        }
                        _ => Err(()),
                    }
                }
                _ => skip_complete_type(chars, pos), // element type
            }
        }
        Some('(') => {
            *pos += 1; // consume '('
            while matches!(chars.get(*pos).copied(), Some(c) if c != ')') {
                skip_complete_type(chars, pos)?;
            }
            match chars.get(*pos).copied() {
                Some(')') => {
                    *pos += 1;
                    Ok(())
                }
                _ => Err(()),
            }
        }
        // Unknown type code — consume it so the caller can keep walking;
        // the result is best-effort anyway.
        Some(_) => {
            *pos += 1;
            Ok(())
        }
    }
}

/// The dbus-send `type:` tag for a single basic type code (`s`→`string`,
/// `u`→`uint32`, …). `None` for complex types (dbus-send can't nest).
fn dbus_send_type_tag(sig: &str) -> Option<&'static str> {
    // A single basic code (not a multi-type signature, not array/struct/dict).
    match sig {
        "s" => Some("string"),
        "u" => Some("uint32"),
        "i" => Some("int32"),
        "t" => Some("uint64"),
        "x" => Some("int64"),
        "n" => Some("int16"),
        "q" => Some("uint16"),
        "y" => Some("byte"),
        "b" => Some("boolean"),
        "d" => Some("double"),
        "o" => Some("objpath"),
        "g" => Some("signature"),
        _ => None,
    }
}

/// Pair each top-level signature type with its positional busctl token(s) for
/// dbus-send rendering. Returns `(note, vec of (type_tag, value))`. `note` is
/// `Some(sig)` if any top-level type is complex (dbus-send can't nest); in
/// that case complex args get a best-effort `variant:<value>` tag.
fn split_for_dbus_send<'a>(
    signature: &'a str,
    args: &[String],
) -> (Option<&'a str>, Vec<(&'static str, String)>) {
    let types = split_signature(signature);
    let mut tokens = Vec::new();
    let mut arg_pos = 0;
    let mut saw_complex = false;
    for ty in &types {
        match dbus_send_type_tag(ty) {
            Some(tag) => {
                let val = args.get(arg_pos).cloned().unwrap_or_default();
                arg_pos += 1;
                tokens.push((tag, val));
            }
            None => {
                // Complex type — consume its busctl token span best-effort.
                // We don't model the positional layout precisely (it varies
                // by type), so drain one token as a representative value and
                // flag the signature.
                let val = args.get(arg_pos).cloned().unwrap_or_default();
                if arg_pos < args.len() {
                    arg_pos += 1;
                }
                tokens.push(("variant", val));
                saw_complex = true;
            }
        }
    }
    let note = if saw_complex { Some(signature) } else { None };
    (note, tokens)
}

/// Build the literal arg list for a qdbus method call: for basic types the
/// raw token; for an array, drop the busctl count prefix and join the
/// element tokens with spaces (best-effort). Complex types best-effort.
fn qdbus_call_args(signature: &str, args: &[String]) -> Vec<String> {
    let types = split_signature(signature);
    let mut out = Vec::new();
    let mut arg_pos = 0;
    for ty in &types {
        if let Some(_tag) = dbus_send_basic_literal_kind(ty) {
            // basic → the raw token (qdbus infers the type).
            let val = args.get(arg_pos).cloned().unwrap_or_default();
            if arg_pos < args.len() {
                arg_pos += 1;
            }
            out.push(val);
        } else if ty.starts_with('a') && !ty.starts_with("a{") {
            // array (non-dict): busctl lays out `count elem0 elem1 …`.
            // Drop the count, join the elements as separate literals.
            let count: usize = args.get(arg_pos).map(|s| s.parse().unwrap_or(0)).unwrap_or(0);
            if arg_pos < args.len() {
                arg_pos += 1;
            }
            for _ in 0..count {
                let val = args.get(arg_pos).cloned().unwrap_or_default();
                if arg_pos < args.len() {
                    arg_pos += 1;
                }
                out.push(val);
            }
        } else {
            // dict / struct / variant — best-effort: emit the next token.
            let val = args.get(arg_pos).cloned().unwrap_or_default();
            if arg_pos < args.len() {
                arg_pos += 1;
            }
            out.push(val);
        }
    }
    // Fallback: if the signature was empty/weird but args exist, emit them raw.
    if out.is_empty() {
        for a in args {
            out.push(a.clone());
        }
    }
    out
}

/// Whether a basic type code maps to a qdbus "literal" value (used to decide
/// basic-vs-array handling). Returns `Some(())` for basic codes.
fn dbus_send_basic_literal_kind(ty: &str) -> Option<()> {
    matches!(
        ty,
        "y" | "b" | "n" | "q" | "i" | "u" | "x" | "t" | "d" | "s" | "o" | "g"
    )
    .then_some(())
}

/// Does `signature` contain any complex top-level type (array/struct/dict)?
fn gdbus_has_complex(signature: &str) -> bool {
    split_signature(signature)
        .iter()
        .any(|ty| dbus_send_type_tag(ty).is_none())
}

/// Render one busctl-style value as GVariant text for a gdbus call argument.
/// Basic types: strings `"..."`, booleans `true`/`false`, numbers bare.
/// Complex types: best-effort (emit the raw token quoted as a string).
fn gdbus_single_value(signature: &str, value: &str) -> String {
    let ty = split_signature(signature).into_iter().next();
    gdbus_value_text(ty.as_deref(), value)
}

/// Render each top-level arg as GVariant text for a gdbus call (method args).
fn gdbus_call_args(signature: &str, args: &[String]) -> Vec<String> {
    let types = split_signature(signature);
    let mut out = Vec::new();
    let mut arg_pos = 0;
    for ty in &types {
        let val = args.get(arg_pos).cloned().unwrap_or_default();
        if arg_pos < args.len() {
            arg_pos += 1;
        }
        out.push(gdbus_value_text(Some(ty), &val));
    }
    out
}

/// Render a single value as GVariant text given its (optional) type.
fn gdbus_value_text(ty: Option<&str>, value: &str) -> String {
    match ty {
        Some("s") | Some("o") | Some("g") => gdbus_string(value),
        Some("b") => gdbus_bool(value),
        Some("y" | "n" | "q" | "i" | "u" | "x" | "t" | "d") => value.into(),
        // complex / unknown / variant → best-effort: quote as a string.
        _ => gdbus_string(value),
    }
}

/// GVariant-text double-quoted string (escapes `\` and `"`).
fn gdbus_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// GVariant-text boolean (qdbus/busctl accept `true`/`false`/`1`/`0`).
fn gdbus_bool(s: &str) -> String {
    match s {
        "true" | "yes" | "on" | "1" => "true".into(),
        _ => "false".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_signature_basics() {
        assert_eq!(split_signature("su"), vec!["s", "u"]);
        assert_eq!(split_signature("as"), vec!["as"]);
        assert_eq!(split_signature("a{sv}"), vec!["a{sv}"]);
        assert_eq!(split_signature("(ii)s"), vec!["(ii)", "s"]);
        assert_eq!(split_signature("a(ii)u"), vec!["a(ii)", "u"]);
        assert_eq!(split_signature(""), Vec::<String>::new());
    }

    #[test]
    fn quote_only_when_needed() {
        assert_eq!(quote("42"), "42");
        assert_eq!(quote("hi"), "hi");
        assert_eq!(quote("a b"), "\"a b\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("type='signal'"), "\"type='signal'\"");
    }
}
