//! Dict rendering with the spec §7.2 safety rule.
//!
//! - **string-key dict** (`a{sv}`, `a{ss}`, `a{os}`, ...) → JSON object:
//!   `{"type":"a{kv}","data":{key: <tagged value>, ...}}`
//! - **non-string-key dict** (`a{uu}`, `a{uv}`, ...) → array of pairs so JSON
//!   never has to invent a string key (this is the sd-bus #32904 crash case):
//!   `{"type":"a{kv}","data":[{"key":<tagged>, "value":<tagged>}, ...]}`
//!
//! Keys stay in their native JSON type (a `u32` 1 stays the number `1`, not the
//! string `"1"`).

use serde_json::{Value as Json, json};
use zvariant::{Dict, Signature, Value};

/// Render a [`Dict`] as type-tagged JSON, applying the string-key rule above.
pub fn to_tagged(d: &Dict) -> Json {
    let sig = d.signature().to_string();
    // The key signature drives the object-vs-array choice. The dict's own
    // signature carries it: `Signature::Dict { key, value }`.
    let key_is_string = matches!(d.signature(), Signature::Dict { key, .. } if is_string_key(key));

    if key_is_string {
        let mut obj = serde_json::Map::new();
        for (k, v) in d.iter() {
            let Some(key) = string_key(k) else {
                // Signature promised a string key but the entry disagrees — fall
                // back to the safe array-of-pairs path for this dict rather than
                // dropping or mis-keying entries.
                return array_of_pairs(d, &sig);
            };
            obj.insert(key, crate::value::decode::to_tagged(v));
        }
        json!({ "type": sig, "data": Json::Object(obj) })
    } else {
        array_of_pairs(d, &sig)
    }
}

fn array_of_pairs(d: &Dict, sig: &str) -> Json {
    let data = d
        .iter()
        .map(|(k, v)| {
            json!({
                "key": crate::value::decode::to_tagged(k),
                "value": crate::value::decode::to_tagged(v),
            })
        })
        .collect::<Vec<_>>();
    json!({ "type": sig, "data": data })
}

/// Whether a dict-key signature is one of the D-Bus string-like types that maps
/// naturally to a JSON object key (`s`, `o`, `g`).
fn is_string_key(key: &Signature) -> bool {
    matches!(key, Signature::Str | Signature::ObjectPath | Signature::Signature)
}

/// Extract a `String` key from a string-like [`Value`]. Returns `None` for any
/// non-string key, so the caller can switch to the safe pair-array path.
fn string_key(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(o) => Some(o.to_string()),
        Value::Signature(s) => Some(s.to_string()),
        _ => None,
    }
}
