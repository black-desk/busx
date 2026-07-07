// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render a [`zvariant::Value`] as type-tagged JSON (spec §7.2).
//!
//! Every value is encoded as `{"type": <signature>, "data": <native JSON>}`.
//! Non-string-key dicts are kept safe — see [`crate::value::dict`].

use serde_json::{Value as Json, json};
use zvariant::Value;

/// Convert a borrowed [`Value`] into type-tagged JSON.
pub fn to_tagged(v: &Value) -> Json {
    match v {
        // --- basic types ---
        Value::U8(b) => tagged("y", json!(b)),
        Value::Bool(b) => tagged("b", json!(b)),
        Value::I16(i) => tagged("n", json!(i)),
        Value::U16(i) => tagged("q", json!(i)),
        Value::I32(i) => tagged("i", json!(i)),
        Value::U32(i) => tagged("u", json!(i)),
        Value::I64(i) => tagged("x", json!(i)),
        Value::U64(i) => tagged("t", json!(i)),
        Value::F64(d) => tagged("d", json!(d)),
        Value::Str(s) => tagged("s", json!(s.as_str())),
        Value::Signature(s) => tagged("g", json!(s.to_string())),
        Value::ObjectPath(o) => tagged("o", json!(o.as_str())),

        // --- container types ---
        Value::Value(inner) => tagged("v", to_tagged(inner)),
        Value::Array(a) => {
            let sig = format!("a{}", a.element_signature());
            let data = Json::Array(a.inner().iter().map(to_tagged).collect());
            tagged(&sig, data)
        }
        Value::Structure(s) => {
            // `Structure::signature()` already yields the canonical `(...)`.
            let sig = s.signature().to_string();
            let data = Json::Array(s.fields().iter().map(to_tagged).collect());
            tagged(&sig, data)
        }
        Value::Dict(d) => crate::value::dict::to_tagged(d),

        // --- optional / platform variants ---
        // `Maybe` is gated behind the `gvariant` feature (not enabled in busx) and
        // `Fd` behind `cfg(unix)`. A catch-all keeps this compiling regardless of
        // which optional variants are present, and degrades gracefully instead of
        // crashing on an unfamiliar type.
        other => tagged("_", json!(format!("unsupported value: {other:?}"))),
    }
}

fn tagged(ty: &str, data: Json) -> Json {
    json!({ "type": ty, "data": data })
}
