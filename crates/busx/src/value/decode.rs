// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render a [`zvariant::Value`] as type-tagged JSON.
//!
//! Every value is encoded as `{"type": <signature>, "data": <native JSON>}`.
//! Non-string-key dicts are kept safe — see [`crate::value::dict`].

use serde_json::{Value as Json, json};
use std::os::fd::AsRawFd;
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

        // Real fd (duplicated into this process). Emit a structured object so
        // consumers get kind/target/mode instead of a meaningless process-local
        // integer. Resolved synchronously; the fd is not held (see `fdinfo`).
        Value::Fd(fd) => {
            let info = crate::value::fdinfo::gather(fd.as_raw_fd());
            let mut obj = serde_json::Map::new();
            obj.insert("kind".into(), json!(info.kind));
            if let Some(t) = info.target {
                obj.insert("target".into(), json!(t));
            }
            if !info.mode.is_empty() {
                obj.insert("mode".into(), json!(info.mode));
            }
            if let Some(size) = info.size {
                obj.insert("size".into(), json!(size));
            }
            if let Some(note) = info.note {
                obj.insert("note".into(), json!(note));
            }
            tagged("h", Json::Object(obj))
        }

        // --- optional / platform variants ---
        // `Maybe` is gated behind the `gvariant` feature (not enabled in busx).
        // A catch-all keeps this compiling regardless of which optional
        // variants are present, and degrades gracefully instead of crashing on
        // an unfamiliar type. (`Fd` is handled above on unix.)
        #[allow(unreachable_patterns)]
        other => tagged("_", json!(format!("unsupported value: {other:?}"))),
    }
}

fn tagged(ty: &str, data: Json) -> Json {
    json!({ "type": ty, "data": data })
}
