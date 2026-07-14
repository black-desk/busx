// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Human-friendly rendering of a [`zvariant::Value`].
//!
//! This is the value pretty-printer used by every command's *default* output
//! (human text). It mirrors the variant walk in [`crate::value::decode`] so the
//! accessor patterns stay in lock-step, but renders a compact, shell-friendly
//! form instead of type-tagged JSON:
//!
//! - numbers (U8/I16/U32/U64/I64/F64) → bare number.
//! - Bool → `true`/`false`.
//! - Str / ObjectPath / Signature → `"..."` (with `"` and `\` escaped).
//! - Variant → `<{inner_type_sig} {pretty(inner)}>` (e.g. `<d 0.5>`).
//! - Array → `[e0, e1, ...]`.
//! - Dict → `{k0: v0, k1: v1, ...}` (works for every key type — the human form
//!   has no JSON object-key limit, so even `a{uu}` renders as `{1:10, 2:20}`).
//! - Structure → `(f0, f1, ...)`.
//! - anything else (Fd, Maybe under a gated feature) → `<?>`.

use zvariant::Value;

/// Render a borrowed [`Value`] as a human-friendly string.
pub fn pretty(v: &Value<'_>) -> String {
    match v {
        Value::U8(b) => b.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::I16(i) => i.to_string(),
        Value::U16(i) => i.to_string(),
        Value::I32(i) => i.to_string(),
        Value::U32(i) => i.to_string(),
        Value::I64(i) => i.to_string(),
        Value::U64(i) => i.to_string(),
        Value::F64(d) => d.to_string(),
        Value::Str(s) => quote_str(s.as_str()),
        Value::Signature(s) => quote_str(&s.to_string()),
        Value::ObjectPath(o) => quote_str(o.as_str()),

        // A variant carries its own inner signature, so render it tag-first.
        // `value_signature()` gives the enclosed type (e.g. `d`), whereas the
        // trait `DynamicType::signature` would always return `v`.
        Value::Value(inner) => format!("<{} {}>", inner.value_signature(), pretty(inner)),
        Value::Array(a) => bracket('[', ']', a.inner().iter().map(pretty)),
        Value::Structure(s) => bracket('(', ')', s.fields().iter().map(pretty)),
        Value::Dict(d) => bracket(
            '{',
            '}',
            d.iter()
                .map(|(k, v)| format!("{}: {}", pretty(k), pretty(v))),
        ),
        // Optional/platform variants: Fd (cfg(unix)), Maybe (gvariant feature).
        // Render a clear placeholder rather than crashing on an unfamiliar type.
        other => format!("unsupported value: {other:?}"),
    }
}

/// Join an iterator of already-rendered element strings between `open`/`close`.
fn bracket<I>(open: char, close: char, items: I) -> String
where
    I: IntoIterator<Item = String>,
{
    let mut s = String::new();
    s.push(open);
    let mut first = true;
    for it in items {
        if !first {
            s.push_str(", ");
        }
        first = false;
        s.push_str(&it);
    }
    s.push(close);
    s
}

/// Quote a string and escape `"` and `\`. Non-string scalars don't need this.
fn quote_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
